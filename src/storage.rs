//! SQLite-backed storage for address mappings.

use crate::error::ProxyError;
use rusqlite::{params, Connection};
use std::path::Path;

/// Result type for storage operations.
pub type Result<T> = std::result::Result<T, ProxyError>;

/// Represents a mapping between an Ethereum address and a Miden AccountId.
#[derive(Debug, Clone)]
pub struct AddressMapping {
    /// Ethereum address (20 bytes)
    pub eth_address: [u8; 20],
    /// Miden AccountId (15 bytes for 120-bit ID)
    pub miden_account_id: [u8; 15],
    /// Timestamp when the mapping was created
    pub created_at: i64,
    /// Whether the account was auto-created
    pub auto_created: bool,
}

/// SQLite-backed storage for address mappings.
pub struct MappingStorage {
    conn: Connection,
}

impl MappingStorage {
    /// Create a new storage instance, initializing the database schema.
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path).map_err(|e| ProxyError::Internal(e.to_string()))?;
        let storage = Self { conn };
        storage.init_schema()?;
        Ok(storage)
    }

    /// Create an in-memory storage instance (useful for testing).
    pub fn in_memory() -> Result<Self> {
        let conn =
            Connection::open_in_memory().map_err(|e| ProxyError::Internal(e.to_string()))?;
        let storage = Self { conn };
        storage.init_schema()?;
        Ok(storage)
    }

    /// Initialize the database schema.
    fn init_schema(&self) -> Result<()> {
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS address_mappings (
                eth_address BLOB PRIMARY KEY NOT NULL,
                miden_account_id BLOB NOT NULL,
                created_at INTEGER NOT NULL,
                auto_created INTEGER NOT NULL
            )",
                [],
            )
            .map_err(|e| ProxyError::Internal(e.to_string()))?;

        // Index for looking up by Miden account ID
        self.conn
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_miden_account_id
             ON address_mappings (miden_account_id)",
                [],
            )
            .map_err(|e| ProxyError::Internal(e.to_string()))?;

        Ok(())
    }

    /// Look up a Miden AccountId by Ethereum address.
    pub fn get_by_eth_address(&self, eth_address: &[u8; 20]) -> Result<Option<AddressMapping>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT eth_address, miden_account_id, created_at, auto_created
             FROM address_mappings
             WHERE eth_address = ?1",
            )
            .map_err(|e| ProxyError::Internal(e.to_string()))?;

        let mut rows = stmt
            .query(params![eth_address.as_slice()])
            .map_err(|e| ProxyError::Internal(e.to_string()))?;

        if let Some(row) = rows.next().map_err(|e| ProxyError::Internal(e.to_string()))? {
            let eth_bytes: Vec<u8> = row.get(0).map_err(|e| ProxyError::Internal(e.to_string()))?;
            let miden_bytes: Vec<u8> =
                row.get(1).map_err(|e| ProxyError::Internal(e.to_string()))?;

            let mut eth_addr = [0u8; 20];
            let mut miden_id = [0u8; 15];
            eth_addr.copy_from_slice(&eth_bytes);
            miden_id.copy_from_slice(&miden_bytes);

            Ok(Some(AddressMapping {
                eth_address: eth_addr,
                miden_account_id: miden_id,
                created_at: row.get(2).map_err(|e| ProxyError::Internal(e.to_string()))?,
                auto_created: row
                    .get::<_, i64>(3)
                    .map_err(|e| ProxyError::Internal(e.to_string()))?
                    != 0,
            }))
        } else {
            Ok(None)
        }
    }

    /// Look up an Ethereum address by Miden AccountId.
    pub fn get_by_miden_id(&self, miden_id: &[u8; 15]) -> Result<Option<AddressMapping>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT eth_address, miden_account_id, created_at, auto_created
             FROM address_mappings
             WHERE miden_account_id = ?1",
            )
            .map_err(|e| ProxyError::Internal(e.to_string()))?;

        let mut rows = stmt
            .query(params![miden_id.as_slice()])
            .map_err(|e| ProxyError::Internal(e.to_string()))?;

        if let Some(row) = rows.next().map_err(|e| ProxyError::Internal(e.to_string()))? {
            let eth_bytes: Vec<u8> = row.get(0).map_err(|e| ProxyError::Internal(e.to_string()))?;
            let miden_bytes: Vec<u8> =
                row.get(1).map_err(|e| ProxyError::Internal(e.to_string()))?;

            let mut eth_addr = [0u8; 20];
            let mut miden_account_id = [0u8; 15];
            eth_addr.copy_from_slice(&eth_bytes);
            miden_account_id.copy_from_slice(&miden_bytes);

            Ok(Some(AddressMapping {
                eth_address: eth_addr,
                miden_account_id,
                created_at: row.get(2).map_err(|e| ProxyError::Internal(e.to_string()))?,
                auto_created: row
                    .get::<_, i64>(3)
                    .map_err(|e| ProxyError::Internal(e.to_string()))?
                    != 0,
            }))
        } else {
            Ok(None)
        }
    }

    /// Insert a new address mapping.
    pub fn insert(&self, mapping: &AddressMapping) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO address_mappings (eth_address, miden_account_id, created_at, auto_created)
             VALUES (?1, ?2, ?3, ?4)",
                params![
                    mapping.eth_address.as_slice(),
                    mapping.miden_account_id.as_slice(),
                    mapping.created_at,
                    mapping.auto_created as i64,
                ],
            )
            .map_err(|e| ProxyError::Internal(e.to_string()))?;
        Ok(())
    }

    /// Check if a mapping exists for the given Ethereum address.
    pub fn exists(&self, eth_address: &[u8; 20]) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM address_mappings WHERE eth_address = ?1 LIMIT 1")
            .map_err(|e| ProxyError::Internal(e.to_string()))?;
        let exists = stmt
            .exists(params![eth_address.as_slice()])
            .map_err(|e| ProxyError::Internal(e.to_string()))?;
        Ok(exists)
    }

    /// Get total count of mappings.
    pub fn count(&self) -> Result<u64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM address_mappings", [], |row| {
                row.get(0)
            })
            .map_err(|e| ProxyError::Internal(e.to_string()))?;
        Ok(count as u64)
    }

    /// List all mappings (for debugging/admin purposes).
    #[allow(dead_code)]
    pub fn list_all(&self) -> Result<Vec<AddressMapping>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT eth_address, miden_account_id, created_at, auto_created
             FROM address_mappings
             ORDER BY created_at DESC",
            )
            .map_err(|e| ProxyError::Internal(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                let eth_bytes: Vec<u8> = row.get(0)?;
                let miden_bytes: Vec<u8> = row.get(1)?;

                let mut eth_addr = [0u8; 20];
                let mut miden_id = [0u8; 15];
                eth_addr.copy_from_slice(&eth_bytes);
                miden_id.copy_from_slice(&miden_bytes);

                Ok(AddressMapping {
                    eth_address: eth_addr,
                    miden_account_id: miden_id,
                    created_at: row.get(2)?,
                    auto_created: row.get::<_, i64>(3)? != 0,
                })
            })
            .map_err(|e| ProxyError::Internal(e.to_string()))?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| ProxyError::Internal(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_roundtrip() {
        let storage = MappingStorage::in_memory().unwrap();

        let mapping = AddressMapping {
            eth_address: [1u8; 20],
            miden_account_id: [2u8; 15],
            created_at: 1234567890,
            auto_created: true,
        };

        storage.insert(&mapping).unwrap();

        let retrieved = storage.get_by_eth_address(&mapping.eth_address).unwrap();
        assert!(retrieved.is_some());

        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.eth_address, mapping.eth_address);
        assert_eq!(retrieved.miden_account_id, mapping.miden_account_id);
        assert_eq!(retrieved.auto_created, mapping.auto_created);
    }

    #[test]
    fn test_get_by_miden_id() {
        let storage = MappingStorage::in_memory().unwrap();

        let mapping = AddressMapping {
            eth_address: [3u8; 20],
            miden_account_id: [4u8; 15],
            created_at: 1234567890,
            auto_created: false,
        };

        storage.insert(&mapping).unwrap();

        let retrieved = storage.get_by_miden_id(&mapping.miden_account_id).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().eth_address, mapping.eth_address);
    }

    #[test]
    fn test_not_found() {
        let storage = MappingStorage::in_memory().unwrap();

        let result = storage.get_by_eth_address(&[0u8; 20]).unwrap();
        assert!(result.is_none());
    }
}
