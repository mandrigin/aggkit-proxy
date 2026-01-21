//! Verify Notes - Query Miden node state and list notes
//!
//! This binary connects to a miden-node, syncs state, and lists available notes.
//! Can also query specific notes by ID directly from the node.
//!
//! Usage:
//!   verify-notes                      # List all tracked notes
//!   verify-notes --note-id <id>       # Query a specific note from the node
//!
//! Environment variables:
//!   MIDEN_RPC_URL - Node RPC endpoint (default: http://localhost:57291)
//!   MIDEN_STORE_PATH - Client store path (default: /tmp/verify-notes-store)

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::notes::NoteFile;
use miden_client::rpc::Endpoint;
use miden_client::store::NoteFilter;
use miden_client::Client;
use miden_client_sqlite_store::SqliteStore;
use miden_protocol::note::NoteId;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("warn"))
        )
        .init();

    // Parse command-line arguments
    let args: Vec<String> = std::env::args().collect();
    let mut note_id_to_query: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--note-id" | "-n" => {
                if i + 1 < args.len() {
                    note_id_to_query = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("Error: --note-id requires an argument");
                    std::process::exit(1);
                }
            }
            "--help" | "-h" => {
                println!("Usage: verify-notes [OPTIONS]");
                println!();
                println!("Options:");
                println!("  --note-id, -n <ID>  Query a specific note from the node by its ID");
                println!("  --help, -h          Show this help");
                println!();
                println!("Environment variables:");
                println!("  MIDEN_RPC_URL       Node RPC endpoint (default: http://localhost:57291)");
                println!("  MIDEN_STORE_PATH    Client store path (default: /tmp/verify-notes-store)");
                return Ok(());
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
                std::process::exit(1);
            }
        }
    }

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

    println!("✓ Synced to block {}", sync_result.block_num.as_u32());
    println!();

    // If querying a specific note by ID
    if let Some(note_id_str) = note_id_to_query {
        println!("═══════════════════════════════════════════════════════════════════");
        println!("                    QUERYING NOTE BY ID                             ");
        println!("═══════════════════════════════════════════════════════════════════");
        println!();
        println!("  Note ID: {}", note_id_str);
        println!();

        // Parse note ID (accepts hex string with or without 0x prefix)
        let note_id_hex = note_id_str.strip_prefix("0x").unwrap_or(&note_id_str);
        let note_id = NoteId::try_from_hex(note_id_hex)
            .map_err(|e| format!("Invalid note ID '{}': {:?}", note_id_str, e))?;

        // Try to import/fetch the note from the node
        println!("Querying node for note...");
        let note_files = vec![NoteFile::NoteId(note_id)];
        match client.import_notes(&note_files).await {
            Ok(imported_ids) => {
                if imported_ids.is_empty() {
                    println!("✗ Note not found on node");
                } else {
                    println!("✓ Note found on node!");
                    println!();
                    for imported_id in &imported_ids {
                        println!("  Imported Note ID: {}", imported_id);
                    }

                    // Try to get the note details from local store now that it's imported
                    let input_notes = client.get_input_notes(NoteFilter::All).await
                        .map_err(|e| format!("Failed to get input notes: {}", e))?;

                    for imported_id in &imported_ids {
                        if let Some(note) = input_notes.iter().find(|n| n.id() == *imported_id) {
                            println!("  State:  {:?}", note.state());
                            let details = note.details();
                            let assets = details.assets();
                            if assets.num_assets() > 0 {
                                println!("  Assets: {} asset(s)", assets.num_assets());
                                for asset in assets.iter() {
                                    println!("    - {:?}", asset);
                                }
                            }
                        }
                    }
                }
                println!();
            }
            Err(e) => {
                println!("✗ Note not found or error fetching:");
                println!("  {}", e);
                println!();
                println!("Note: The node only returns public notes or notes you have");
                println!("the proper authentication to access.");
            }
        }

        println!();
        println!("✓ Query complete");
        return Ok(());
    }

    // Get input notes (notes we can consume)
    println!("═══════════════════════════════════════════════════════════════════");
    println!("                         INPUT NOTES                                ");
    println!("═══════════════════════════════════════════════════════════════════");
    println!();

    let input_notes = client.get_input_notes(NoteFilter::All).await
        .map_err(|e| format!("Failed to get input notes: {}", e))?;

    if input_notes.is_empty() {
        println!("  (No input notes tracked by this client)");
    } else {
        for (i, note) in input_notes.iter().enumerate() {
            println!("  Note #{}:", i + 1);
            println!("    ID:     {}", note.id());
            println!("    State:  {:?}", note.state());
            // Try to get assets if available
            let details = note.details();
            let assets = details.assets();
            if assets.num_assets() > 0 {
                println!("    Assets: {} asset(s)", assets.num_assets());
                for asset in assets.iter() {
                    println!("      - {:?}", asset);
                }
            }
            println!();
        }
    }

    // Get output notes (notes we created)
    println!("═══════════════════════════════════════════════════════════════════");
    println!("                         OUTPUT NOTES                               ");
    println!("═══════════════════════════════════════════════════════════════════");
    println!();

    let output_notes = client.get_output_notes(NoteFilter::All).await
        .map_err(|e| format!("Failed to get output notes: {}", e))?;

    if output_notes.is_empty() {
        println!("  (No output notes created by this client)");
    } else {
        for (i, note) in output_notes.iter().enumerate() {
            println!("  Note #{}:", i + 1);
            println!("    ID:     {}", note.id());
            println!("    State:  {:?}", note.state());
            let assets = note.assets();
            if assets.num_assets() > 0 {
                println!("    Assets: {} asset(s)", assets.num_assets());
                for asset in assets.iter() {
                    println!("      - {:?}", asset);
                }
            }
            println!();
        }
    }

    // Summary
    println!("═══════════════════════════════════════════════════════════════════");
    println!("                           SUMMARY                                  ");
    println!("═══════════════════════════════════════════════════════════════════");
    println!();
    println!("  Node:         Connected ({})", rpc_url);
    println!("  Block Height: {}", sync_result.block_num.as_u32());
    println!("  Input Notes:  {}", input_notes.len());
    println!("  Output Notes: {}", output_notes.len());
    println!();
    println!("✓ Verification complete");
    println!();

    Ok(())
}
