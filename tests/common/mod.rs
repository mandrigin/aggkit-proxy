//! Common test utilities for Miden integration tests
//!
//! Uses miden-client from agglayer-v0.1 tag.

use miden_client::{Client, ClientError};
use miden_client_sqlite_store::SqliteStore;
use std::env;
use std::path::PathBuf;

/// Get Miden node URL from environment or use default
pub fn get_node_url() -> String {
    env::var("MIDEN_NODE_URL").unwrap_or_else(|_| "http://localhost:57291".to_string())
}

/// Get a unique test database path
pub fn get_test_db_path() -> PathBuf {
    let test_id = std::process::id();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    let temp_dir = env::temp_dir();
    temp_dir.join(format!("miden_test_{}_{}.db", test_id, timestamp))
}

/// Create a test client connected to the Miden node
pub async fn create_test_client() -> Result<Client<SqliteStore>, ClientError> {
    let node_url = get_node_url();
    let db_path = get_test_db_path();

    // Create SQLite store for test
    let store = SqliteStore::new(db_path).await?;

    // Create client with RPC endpoint
    Client::new(store, &node_url).await
}

/// Cleanup test database
pub fn cleanup_db(path: PathBuf) {
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
}
