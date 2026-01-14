//! Common test utilities for Miden integration tests

use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::rpc::Endpoint;
use miden_client::Client;
use miden_client_sqlite_store::SqliteStore;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

/// Get Miden node URL from environment or use default
pub fn get_node_url() -> String {
    env::var("MIDEN_NODE_URL").unwrap_or_else(|_| "http://localhost:57291".to_string())
}

/// State holder for test client (keeps temp dir alive)
pub struct TestClientState {
    #[allow(dead_code)]
    temp_dir: TempDir,
}

/// Create a test client connected to the Miden node
pub async fn create_test_client() -> (Client<FilesystemKeyStore>, TestClientState) {
    let node_url = get_node_url();

    // Create temp directory for store and keystore
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store_path = temp_dir.path().to_path_buf();

    // Initialize SQLite store
    let store = SqliteStore::new(store_path.clone())
        .await
        .expect("Failed to create SQLite store");

    // Parse endpoint
    let endpoint = Endpoint::try_from(node_url.as_str()).expect("Invalid node URL");

    // Create keystore path
    let keystore_path = store_path.join("keystore");
    let keystore_path_str = keystore_path.to_string_lossy();

    // Build client using new builder pattern
    let client: Client<FilesystemKeyStore> = ClientBuilder::new()
        .grpc_client(&endpoint, Some(10_000))
        .store(Arc::new(store))
        .filesystem_keystore(&keystore_path_str)
        .build()
        .await
        .expect("Failed to create client");

    let state = TestClientState { temp_dir };

    (client, state)
}
