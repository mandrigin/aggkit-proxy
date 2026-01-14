//! Address mapping between Ethereum and Miden addresses.
//!
//! Implements "Option 3: Auto-create accounts" from the spec:
//! - When a claim arrives for an unknown Ethereum address, auto-create a Miden wallet
//! - Use deterministic derivation: eth_address -> keccak256 -> seed -> AccountId

use crate::error::ProxyError;
use crate::storage::{AddressMapping, MappingStorage};
use sha3::{Digest, Keccak256};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

/// Result type for address mapper operations.
pub type Result<T> = std::result::Result<T, ProxyError>;

/// Configuration for the AddressMapper.
#[derive(Debug, Clone)]
pub struct AddressMapperConfig {
    /// Domain separator for deterministic derivation to avoid collisions
    pub domain_separator: Vec<u8>,
}

impl Default for AddressMapperConfig {
    fn default() -> Self {
        Self {
            domain_separator: b"aggkit-proxy-eth-to-miden-v1".to_vec(),
        }
    }
}

/// Maps Ethereum addresses (160-bit) to Miden AccountIds (120-bit).
///
/// Uses SQLite for persistent storage and supports:
/// - Explicit registration of address mappings
/// - Auto-creation of Miden accounts for unknown addresses
/// - Deterministic derivation of seeds from Ethereum addresses
pub struct AddressMapper {
    storage: MappingStorage,
    config: AddressMapperConfig,
}

impl AddressMapper {
    /// Create a new AddressMapper with SQLite storage at the given path.
    pub fn new<P: AsRef<Path>>(db_path: P, config: AddressMapperConfig) -> Result<Self> {
        let storage = MappingStorage::new(db_path)?;
        Ok(Self { storage, config })
    }

    /// Create an in-memory AddressMapper (useful for testing).
    pub fn in_memory(config: AddressMapperConfig) -> Result<Self> {
        let storage = MappingStorage::in_memory()?;
        Ok(Self { storage, config })
    }

    /// Look up a Miden AccountId for an Ethereum address.
    ///
    /// Returns `None` if no mapping exists.
    pub fn lookup(&self, eth_address: &EthAddress) -> Result<Option<MidenAccountId>> {
        if let Some(mapping) = self.storage.get_by_eth_address(&eth_address.0)? {
            Ok(Some(MidenAccountId(mapping.miden_account_id)))
        } else {
            Ok(None)
        }
    }

    /// Get or create a Miden AccountId for an Ethereum address.
    ///
    /// If the address is unknown, derives a seed deterministically and creates
    /// a new Miden wallet account, storing the mapping for future lookups.
    ///
    /// Returns `(account_id, was_created)` where `was_created` is true if a new
    /// account was auto-created.
    pub fn get_or_create(&self, eth_address: &EthAddress) -> Result<(MidenAccountId, bool)> {
        // Check if mapping already exists
        if let Some(mapping) = self.storage.get_by_eth_address(&eth_address.0)? {
            debug!(
                eth = %eth_address,
                miden = %MidenAccountId(mapping.miden_account_id),
                "Found existing mapping"
            );
            return Ok((MidenAccountId(mapping.miden_account_id), false));
        }

        // Derive seed deterministically from Ethereum address
        let seed = self.derive_seed(eth_address);
        info!(
            eth = %eth_address,
            "Deriving new Miden account from Ethereum address"
        );

        // Create the Miden account
        // Note: In production, this would call miden-client to create the actual account.
        // For now, we derive a placeholder AccountId from the seed.
        let miden_account_id = self.seed_to_account_id(&seed)?;

        // Store the mapping
        let mapping = AddressMapping {
            eth_address: eth_address.0,
            miden_account_id: miden_account_id.0,
            created_at: current_timestamp(),
            auto_created: true,
        };
        self.storage.insert(&mapping)?;

        info!(
            eth = %eth_address,
            miden = %miden_account_id,
            "Auto-created new Miden account mapping"
        );

        Ok((miden_account_id, true))
    }

    /// Register an explicit mapping between an Ethereum address and Miden AccountId.
    ///
    /// This is used when the user already has a Miden account and wants to link it
    /// to their Ethereum address.
    pub fn register(
        &self,
        eth_address: &EthAddress,
        miden_account_id: &MidenAccountId,
    ) -> Result<()> {
        if self.storage.exists(&eth_address.0)? {
            warn!(
                eth = %eth_address,
                "Attempted to register address that already has a mapping"
            );
            return Err(ProxyError::AccountResolution {
                eth_address: eth_address.to_string(),
                reason: "address already has a mapping".to_string(),
            });
        }

        let mapping = AddressMapping {
            eth_address: eth_address.0,
            miden_account_id: miden_account_id.0,
            created_at: current_timestamp(),
            auto_created: false,
        };
        self.storage.insert(&mapping)?;

        info!(
            eth = %eth_address,
            miden = %miden_account_id,
            "Registered explicit address mapping"
        );

        Ok(())
    }

    /// Reverse lookup: find the Ethereum address for a Miden AccountId.
    pub fn reverse_lookup(&self, miden_id: &MidenAccountId) -> Result<Option<EthAddress>> {
        if let Some(mapping) = self.storage.get_by_miden_id(&miden_id.0)? {
            Ok(Some(EthAddress(mapping.eth_address)))
        } else {
            Ok(None)
        }
    }

    /// Derive a seed deterministically from an Ethereum address.
    ///
    /// Uses keccak256 with a domain separator to avoid collisions with other
    /// derivation schemes.
    pub fn derive_seed(&self, eth_address: &EthAddress) -> [u8; 32] {
        let mut hasher = Keccak256::new();
        hasher.update(&self.config.domain_separator);
        hasher.update(&eth_address.0);
        hasher.finalize().into()
    }

    /// Convert a 32-byte seed to a 120-bit (15-byte) Miden AccountId.
    ///
    /// Note: In production, this would use miden-client's account creation API
    /// with the seed as input to RpoRandomCoin. For now, we truncate the hash
    /// to 15 bytes as a placeholder.
    fn seed_to_account_id(&self, seed: &[u8; 32]) -> Result<MidenAccountId> {
        // Hash the seed again to get the account ID bytes
        // This ensures we have a clean separation between the seed and the ID
        let mut hasher = Keccak256::new();
        hasher.update(b"miden-account-id-v1");
        hasher.update(seed);
        let hash = hasher.finalize();

        // Take the first 15 bytes for the 120-bit AccountId
        let mut account_id = [0u8; 15];
        account_id.copy_from_slice(&hash[..15]);

        // TODO: In production, set proper account type bits in the ID
        // Miden AccountIds encode type, storage mode, and version in the ID itself

        Ok(MidenAccountId(account_id))
    }

    /// Get total number of mappings stored.
    pub fn count(&self) -> Result<u64> {
        self.storage.count()
    }
}

/// Ethereum address (160-bit, 20 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EthAddress(pub [u8; 20]);

impl EthAddress {
    /// Create from a byte slice.
    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 20 {
            return Err(ProxyError::AccountResolution {
                eth_address: hex::encode(bytes),
                reason: format!("invalid address length: expected 20 bytes, got {}", bytes.len()),
            });
        }
        let mut addr = [0u8; 20];
        addr.copy_from_slice(bytes);
        Ok(Self(addr))
    }

    /// Create from a hex string (with or without 0x prefix).
    pub fn from_hex(hex_str: &str) -> Result<Self> {
        let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
        let bytes = hex::decode(hex_str).map_err(|e| ProxyError::AccountResolution {
            eth_address: hex_str.to_string(),
            reason: format!("invalid hex: {}", e),
        })?;
        Self::from_slice(&bytes)
    }

    /// Create from an alloy Address type.
    pub fn from_alloy(addr: &alloy_primitives::Address) -> Self {
        Self(addr.0 .0)
    }

    /// Convert to an alloy Address type.
    pub fn to_alloy(&self) -> alloy_primitives::Address {
        alloy_primitives::Address::from_slice(&self.0)
    }
}

impl std::fmt::Display for EthAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

impl From<alloy_primitives::Address> for EthAddress {
    fn from(addr: alloy_primitives::Address) -> Self {
        Self::from_alloy(&addr)
    }
}

impl From<EthAddress> for alloy_primitives::Address {
    fn from(addr: EthAddress) -> Self {
        addr.to_alloy()
    }
}

/// Miden AccountId (120-bit, 15 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MidenAccountId(pub [u8; 15]);

impl MidenAccountId {
    /// Create from a byte slice.
    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 15 {
            return Err(ProxyError::AccountResolution {
                eth_address: "unknown".to_string(),
                reason: format!(
                    "invalid Miden AccountId: expected 15 bytes, got {}",
                    bytes.len()
                ),
            });
        }
        let mut id = [0u8; 15];
        id.copy_from_slice(bytes);
        Ok(Self(id))
    }
}

impl std::fmt::Display for MidenAccountId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> AddressMapperConfig {
        AddressMapperConfig::default()
    }

    #[test]
    fn test_get_or_create_new_address() {
        let mapper = AddressMapper::in_memory(test_config()).unwrap();

        let eth_addr = EthAddress([1u8; 20]);
        let (miden_id, was_created) = mapper.get_or_create(&eth_addr).unwrap();

        assert!(was_created);
        assert_eq!(mapper.count().unwrap(), 1);

        // Second call should return the same ID
        let (miden_id2, was_created2) = mapper.get_or_create(&eth_addr).unwrap();
        assert!(!was_created2);
        assert_eq!(miden_id, miden_id2);
    }

    #[test]
    fn test_deterministic_derivation() {
        let mapper1 = AddressMapper::in_memory(test_config()).unwrap();
        let mapper2 = AddressMapper::in_memory(test_config()).unwrap();

        let eth_addr = EthAddress([42u8; 20]);

        // Both mappers should derive the same seed and account ID
        let seed1 = mapper1.derive_seed(&eth_addr);
        let seed2 = mapper2.derive_seed(&eth_addr);
        assert_eq!(seed1, seed2);

        let (id1, _) = mapper1.get_or_create(&eth_addr).unwrap();
        let (id2, _) = mapper2.get_or_create(&eth_addr).unwrap();
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_explicit_registration() {
        let mapper = AddressMapper::in_memory(test_config()).unwrap();

        let eth_addr = EthAddress([5u8; 20]);
        let miden_id = MidenAccountId([6u8; 15]);

        mapper.register(&eth_addr, &miden_id).unwrap();

        let retrieved = mapper.lookup(&eth_addr).unwrap();
        assert_eq!(retrieved, Some(miden_id));
    }

    #[test]
    fn test_reverse_lookup() {
        let mapper = AddressMapper::in_memory(test_config()).unwrap();

        let eth_addr = EthAddress([7u8; 20]);
        let (miden_id, _) = mapper.get_or_create(&eth_addr).unwrap();

        let reverse = mapper.reverse_lookup(&miden_id).unwrap();
        assert_eq!(reverse, Some(eth_addr));
    }

    #[test]
    fn test_different_addresses_different_ids() {
        let mapper = AddressMapper::in_memory(test_config()).unwrap();

        let eth1 = EthAddress([1u8; 20]);
        let eth2 = EthAddress([2u8; 20]);

        let (id1, _) = mapper.get_or_create(&eth1).unwrap();
        let (id2, _) = mapper.get_or_create(&eth2).unwrap();

        assert_ne!(id1, id2);
    }

    #[test]
    fn test_from_hex() {
        let addr = EthAddress::from_hex("0x742d35Cc6634C0532925a3b844Bc9e7595f41111").unwrap();
        assert_eq!(addr.0[0], 0x74);
        assert_eq!(addr.0[19], 0x11);

        // Without 0x prefix
        let addr2 = EthAddress::from_hex("742d35Cc6634C0532925a3b844Bc9e7595f41111").unwrap();
        assert_eq!(addr, addr2);
    }

    #[test]
    fn test_alloy_conversion() {
        let alloy_addr =
            alloy_primitives::Address::from_slice(&[0xab; 20]);
        let eth_addr = EthAddress::from_alloy(&alloy_addr);
        assert_eq!(eth_addr.0, [0xab; 20]);

        let back = eth_addr.to_alloy();
        assert_eq!(back, alloy_addr);
    }
}
