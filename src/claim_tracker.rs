//! Claim tracking and deduplication for replay attack prevention.
//!
//! This module provides thread-safe tracking of claimed global indices
//! to prevent replay attacks. Once a claim is processed, its global index
//! is recorded and subsequent claims with the same index are rejected.

use alloy_primitives::U256;
use dashmap::DashSet;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::{ProxyError, ProxyResult};

/// Thread-safe tracker for claimed global indices.
///
/// Uses a `DashSet` for lock-free concurrent access, allowing multiple
/// RPC handlers to check and record claims simultaneously without blocking.
#[derive(Debug, Clone)]
pub struct ClaimTracker {
    /// Set of claimed global indices.
    claimed: Arc<DashSet<U256>>,
    /// Optional path for persistence.
    persistence_path: Option<PathBuf>,
}

/// Serializable representation of claimed indices for persistence.
#[derive(Debug, Serialize, Deserialize)]
struct PersistedClaims {
    /// Claimed global indices as hex strings.
    claimed_indices: Vec<String>,
}

impl ClaimTracker {
    /// Creates a new claim tracker with optional persistence.
    ///
    /// If `persistence_path` is provided, the tracker will load existing
    /// claims from the file and persist new claims when added.
    ///
    /// # Errors
    ///
    /// Returns an error if the persistence file exists but cannot be read
    /// or parsed.
    pub fn new(persistence_path: Option<PathBuf>) -> ProxyResult<Self> {
        let claimed = Arc::new(DashSet::new());

        let tracker = Self {
            claimed,
            persistence_path,
        };

        // Load existing claims if persistence is enabled
        if let Some(ref path) = tracker.persistence_path {
            if path.exists() {
                tracker.load_from_file(path)?;
            }
        }

        Ok(tracker)
    }

    /// Creates a new in-memory claim tracker without persistence.
    pub fn in_memory() -> Self {
        Self {
            claimed: Arc::new(DashSet::new()),
            persistence_path: None,
        }
    }

    /// Checks if a global index has already been claimed.
    pub fn is_claimed(&self, global_index: &U256) -> bool {
        self.claimed.contains(global_index)
    }

    /// Attempts to claim a global index.
    ///
    /// This is an atomic operation that checks if the index is already
    /// claimed and marks it as claimed if not. Returns `Ok(())` if the
    /// claim was successful, or `Err(AlreadyClaimed)` if the index was
    /// already claimed.
    ///
    /// # Errors
    ///
    /// Returns `ProxyError::AlreadyClaimed` if the global index has
    /// already been processed.
    pub fn try_claim(&self, global_index: U256) -> ProxyResult<()> {
        // insert() returns true if the value was newly inserted
        if self.claimed.insert(global_index) {
            // Persist after successful claim
            if let Some(ref path) = self.persistence_path {
                self.persist_to_file(path)?;
            }
            Ok(())
        } else {
            Err(ProxyError::AlreadyClaimed { global_index })
        }
    }

    /// Removes a claim from the tracker (rollback on failure).
    ///
    /// This should be called when a claim submission fails after `try_claim`
    /// succeeded, to allow the claim to be retried.
    pub fn unclaim(&self, global_index: &U256) {
        if self.claimed.remove(global_index).is_some() {
            // Persist after removal
            if let Some(ref path) = self.persistence_path {
                if let Err(e) = self.persist_to_file(path) {
                    tracing::error!(
                        error = %e,
                        global_index = %global_index,
                        "Failed to persist claim removal"
                    );
                }
            }
            tracing::info!(
                global_index = %global_index,
                "Claim rolled back (removed from tracker)"
            );
        }
    }

    /// Returns the number of claimed indices.
    pub fn len(&self) -> usize {
        self.claimed.len()
    }

    /// Returns true if no claims have been recorded.
    pub fn is_empty(&self) -> bool {
        self.claimed.is_empty()
    }

    /// Loads claimed indices from a persistence file.
    fn load_from_file<P: AsRef<Path>>(&self, path: P) -> ProxyResult<()> {
        let content = std::fs::read_to_string(path.as_ref()).map_err(|e| {
            ProxyError::Internal(format!(
                "failed to read claim persistence file '{}': {}",
                path.as_ref().display(),
                e
            ))
        })?;

        let persisted: PersistedClaims = serde_json::from_str(&content).map_err(|e| {
            ProxyError::Internal(format!(
                "failed to parse claim persistence file '{}': {}",
                path.as_ref().display(),
                e
            ))
        })?;

        for hex_index in persisted.claimed_indices {
            let index = parse_u256_hex(&hex_index).map_err(|e| {
                ProxyError::Internal(format!("invalid global index in persistence file: {}", e))
            })?;
            self.claimed.insert(index);
        }

        tracing::info!(
            count = self.claimed.len(),
            path = %path.as_ref().display(),
            "loaded claimed indices from persistence file"
        );

        Ok(())
    }

    /// Persists all claimed indices to a file.
    fn persist_to_file<P: AsRef<Path>>(&self, path: P) -> ProxyResult<()> {
        let claimed_indices: Vec<String> = self
            .claimed
            .iter()
            .map(|index| format!("0x{:064x}", *index))
            .collect();

        let persisted = PersistedClaims { claimed_indices };

        let content = serde_json::to_string_pretty(&persisted).map_err(|e| {
            ProxyError::Internal(format!("failed to serialize claimed indices: {}", e))
        })?;

        // Write atomically using temp file
        let temp_path = path.as_ref().with_extension("tmp");
        std::fs::write(&temp_path, &content).map_err(|e| {
            ProxyError::Internal(format!(
                "failed to write claim persistence file '{}': {}",
                temp_path.display(),
                e
            ))
        })?;

        std::fs::rename(&temp_path, path.as_ref()).map_err(|e| {
            ProxyError::Internal(format!(
                "failed to rename claim persistence file: {}",
                e
            ))
        })?;

        Ok(())
    }
}

/// Parses a U256 from a hex string (with or without 0x prefix).
fn parse_u256_hex(s: &str) -> Result<U256, String> {
    let s = s.trim().strip_prefix("0x").unwrap_or(s);
    U256::from_str_radix(s, 16).map_err(|e| format!("invalid hex: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use tempfile::tempdir;

    #[test]
    fn test_basic_claim_tracking() {
        let tracker = ClaimTracker::in_memory();

        let index1 = U256::from(12345u64);
        let index2 = U256::from(67890u64);

        // First claim should succeed
        assert!(!tracker.is_claimed(&index1));
        assert!(tracker.try_claim(index1).is_ok());
        assert!(tracker.is_claimed(&index1));

        // Second claim with same index should fail
        let result = tracker.try_claim(index1);
        assert!(matches!(result, Err(ProxyError::AlreadyClaimed { .. })));

        // Different index should succeed
        assert!(tracker.try_claim(index2).is_ok());
        assert!(tracker.is_claimed(&index2));

        assert_eq!(tracker.len(), 2);
    }

    #[test]
    fn test_concurrent_claims() {
        let tracker = ClaimTracker::in_memory();
        let tracker = Arc::new(tracker);
        let success_count = Arc::new(AtomicUsize::new(0));

        // Try to claim the same index from multiple threads
        let index = U256::from(42u64);
        let mut handles = vec![];

        for _ in 0..10 {
            let tracker = Arc::clone(&tracker);
            let success_count = Arc::clone(&success_count);

            let handle = thread::spawn(move || {
                if tracker.try_claim(index).is_ok() {
                    success_count.fetch_add(1, Ordering::SeqCst);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Only one thread should have succeeded
        assert_eq!(success_count.load(Ordering::SeqCst), 1);
        assert!(tracker.is_claimed(&index));
    }

    #[test]
    fn test_persistence() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("claims.json");

        // Create tracker and add claims
        {
            let tracker = ClaimTracker::new(Some(path.clone())).unwrap();
            tracker.try_claim(U256::from(100u64)).unwrap();
            tracker.try_claim(U256::from(200u64)).unwrap();
            tracker.try_claim(U256::from(300u64)).unwrap();
        }

        // Create new tracker and verify claims are loaded
        {
            let tracker = ClaimTracker::new(Some(path.clone())).unwrap();
            assert_eq!(tracker.len(), 3);
            assert!(tracker.is_claimed(&U256::from(100u64)));
            assert!(tracker.is_claimed(&U256::from(200u64)));
            assert!(tracker.is_claimed(&U256::from(300u64)));

            // New claim should also persist
            tracker.try_claim(U256::from(400u64)).unwrap();
        }

        // Verify new claim persisted
        {
            let tracker = ClaimTracker::new(Some(path)).unwrap();
            assert_eq!(tracker.len(), 4);
            assert!(tracker.is_claimed(&U256::from(400u64)));
        }
    }

    #[test]
    fn test_large_global_index() {
        let tracker = ClaimTracker::in_memory();

        // Test with a large 256-bit value
        // mainnetFlag=1, rollupIndex=0xFFFFFFFF, localRootIndex=0xFFFFFFFF
        let large_index = (U256::from(1u128) << 64)
            | (U256::from(u32::MAX) << 32)
            | U256::from(u32::MAX);

        assert!(tracker.try_claim(large_index).is_ok());
        assert!(tracker.is_claimed(&large_index));

        // Duplicate should fail
        assert!(matches!(
            tracker.try_claim(large_index),
            Err(ProxyError::AlreadyClaimed { .. })
        ));
    }

    #[test]
    fn test_parse_u256_hex() {
        assert_eq!(parse_u256_hex("0x0").unwrap(), U256::ZERO);
        assert_eq!(parse_u256_hex("0x1").unwrap(), U256::from(1));
        assert_eq!(parse_u256_hex("1").unwrap(), U256::from(1));
        assert_eq!(parse_u256_hex("0xff").unwrap(), U256::from(255));
        assert_eq!(parse_u256_hex("FF").unwrap(), U256::from(255));

        // Large value
        let large = parse_u256_hex(
            "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        )
        .unwrap();
        assert_eq!(large, U256::MAX);
    }
}
