//! Common test utilities for Miden integration tests
//!
//! Uses miden-client from agglayer-v0.1 tag with ClientBuilder pattern.

use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::rpc::Endpoint;
use miden_client::Client;
use miden_client_sqlite_store::SqliteStore;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

/// Test client type using FilesystemKeyStore for authentication
pub type TestClient = Client<FilesystemKeyStore>;

/// Test error type for convenience
pub type TestError = Box<dyn std::error::Error + Send + Sync>;

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
    temp_dir.join(format!("miden_test_{}_{}", test_id, timestamp))
}

/// Create a test client connected to the Miden node
pub async fn create_test_client() -> Result<(TestClient, FilesystemKeyStore, PathBuf), TestError> {
    let node_url = get_node_url();
    let db_path = get_test_db_path();

    // Create SQLite store for test
    let store = SqliteStore::new(db_path.join("store.db")).await?;

    // Create keystore directory
    let keystore_path = db_path.join("keystore");
    std::fs::create_dir_all(&keystore_path)?;

    // Create keystore instance for direct key management (takes PathBuf)
    let keystore = FilesystemKeyStore::new(keystore_path.clone())?;

    // Parse RPC endpoint
    let endpoint = Endpoint::try_from(node_url.as_str())?;

    // Convert keystore path to string for ClientBuilder
    let keystore_path_str = keystore_path.to_string_lossy().to_string();

    // Build client using new ClientBuilder pattern
    let client: TestClient = ClientBuilder::new()
        .grpc_client(&endpoint, Some(10_000))
        .store(Arc::new(store))
        .filesystem_keystore(&keystore_path_str)
        .build()
        .await?;

    Ok((client, keystore, db_path))
}

/// Cleanup test database
#[allow(dead_code)]
pub fn cleanup_db(path: PathBuf) {
    let _ = std::fs::remove_dir_all(&path);
}
