//! Common test utilities for Miden integration tests

use miden_client::client::Client;
use miden_client::config::{ClientConfig, RpcConfig};
use miden_client::store::sqlite_store::SqliteStore;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use std::env;

/// Get Miden node URL from environment or use default
pub fn get_node_url() -> String {
    env::var("MIDEN_NODE_URL").unwrap_or_else(|_| "http://localhost:57291".to_string())
}

/// Create a test client connected to the Miden node
pub async fn create_test_client() -> Client<SqliteStore, ChaCha20Rng> {
    let node_url = get_node_url();
    let config = ClientConfig {
        rpc: RpcConfig {
            endpoint: node_url.parse().expect("Invalid node URL"),
            ..Default::default()
        },
        ..Default::default()
    };

    let rng = ChaCha20Rng::from_entropy();
    Client::new(config, rng).await.expect("Failed to create client")
}
