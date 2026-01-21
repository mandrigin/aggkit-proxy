//! Verify Notes - Query Miden node state
//!
//! This binary connects to a miden-node, syncs state, and shows current block info.
//! Useful for verifying node connectivity and state.
//!
//! Usage:
//!   verify-notes
//!
//! Environment variables:
//!   MIDEN_RPC_URL - Node RPC endpoint (default: http://localhost:57291)
//!   MIDEN_STORE_PATH - Client store path (default: /tmp/verify-notes-store)

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::rpc::Endpoint;
use miden_client::Client;
use miden_client_sqlite_store::SqliteStore;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .init();

    // Parse configuration from environment
    let rpc_url = std::env::var("MIDEN_RPC_URL")
        .unwrap_or_else(|_| "http://localhost:57291".to_string());
    let store_path = std::env::var("MIDEN_STORE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/verify-notes-store"));

    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║               Miden Node Verification Tool                        ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Configuration:");
    println!("  RPC URL:    {}", rpc_url);
    println!("  Store Path: {}", store_path.display());
    println!();

    // Create store directory if needed
    std::fs::create_dir_all(&store_path)?;
    let keystore_path = store_path.join("keystore");
    std::fs::create_dir_all(&keystore_path)?;

    // Initialize client
    println!("Initializing Miden client...");

    let endpoint = Endpoint::try_from(rpc_url.as_str())
        .map_err(|e| format!("Invalid endpoint: {}", e))?;

    let store_file = store_path.join("store.sqlite3");
    let store = SqliteStore::new(store_file)
        .await
        .map_err(|e| format!("Failed to create store: {}", e))?;

    let keystore = FilesystemKeyStore::new(keystore_path)
        .map_err(|e| format!("Failed to create keystore: {}", e))?;
    let keystore = Arc::new(keystore);

    let mut client: Client<FilesystemKeyStore> = ClientBuilder::new()
        .grpc_client(&endpoint, Some(10_000))
        .store(Arc::new(store))
        .authenticator(keystore)
        .build()
        .await
        .map_err(|e| format!("Failed to build client: {}", e))?;

    println!("✓ Client initialized");
    println!();

    // Sync state to get latest info
    println!("Syncing state with node...");
    let sync_result = client.sync_state().await
        .map_err(|e| format!("Sync failed: {}", e))?;

    println!();
    println!("═══════════════════════════════════════════════════════════════════");
    println!("                           SUMMARY                                  ");
    println!("═══════════════════════════════════════════════════════════════════");
    println!();
    println!("  Node:            Connected ({})", rpc_url);
    println!("  Block Height:    {}", sync_result.block_num.as_u32());
    println!();
    println!("✓ Node verification complete");
    println!();

    Ok(())
}
