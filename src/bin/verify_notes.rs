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
use miden_protocol::asset::Asset;
use miden_protocol::note::NoteId;
use tracing_subscriber::EnvFilter;

use miden_client::store::InputNoteRecord;

/// Format a NoteId as hex with 0x prefix
fn format_note_id(id: &NoteId) -> String {
    // NoteId's Display impl shows hex
    format!("{}", id)
}

/// Format an asset in a human-readable format
fn format_asset(asset: &Asset) -> String {
    match asset {
        Asset::Fungible(fungible) => {
            format!(
                "Fungible {{ faucet: {}, amount: {} }}",
                fungible.faucet_id(),
                fungible.amount()
            )
        }
        Asset::NonFungible(non_fungible) => {
            format!(
                "NonFungible {{ faucet_prefix: {:?} }}",
                non_fungible.faucet_id_prefix()
            )
        }
    }
}

/// Format note state in a user-friendly way
fn format_note_state(note: &InputNoteRecord) -> String {
    let state = note.state();
    let state_name = format!("{:?}", state);
    // Extract just the variant name (e.g., "Committed" from "Committed(...)")
    let variant = state_name.split('(').next().unwrap_or(&state_name);

    // Try to get block number if committed
    if let Some(proof) = note.inclusion_proof() {
        format!("{} (block {})", variant, proof.location().block_num().as_u32())
    } else {
        variant.to_string()
    }
}

/// Print detailed note information
fn print_note_details(note: &InputNoteRecord, indent: &str) {
    println!("{}ID:      {}", indent, format_note_id(&note.id()));
    println!("{}State:   {}", indent, format_note_state(note));

    // Print metadata if available
    if let Some(metadata) = note.metadata() {
        println!("{}Sender:  {}", indent, metadata.sender());
        println!("{}Type:    {:?}", indent, metadata.note_type());
        println!("{}Tag:     0x{:08x}", indent, metadata.tag().as_u32());
    }

    // Print assets
    let details = note.details();
    let assets = details.assets();
    if assets.num_assets() > 0 {
        println!("{}Assets:  {} asset(s)", indent, assets.num_assets());
        for asset in assets.iter() {
            println!("{}  - {}", indent, format_asset(&asset));
        }
    } else {
        println!("{}Assets:  (none)", indent);
    }

    // Print note inputs (important for agglayer notes)
    let storage = details.storage();
    let storage_items = storage.items();
    if !storage_items.is_empty() {
        println!("{}Storage: {} item(s)", indent, storage_items.len());
        for (i, value) in storage_items.iter().enumerate() {
            println!("{}  [{}]: 0x{:016x}", indent, i, value.as_int());
        }
    }

    // Print script root (hash)
    let recipient = details.recipient();
    let script = recipient.script();
    let root = script.root();
    println!("{}Script:  0x{:016x}{:016x}{:016x}{:016x}", indent,
        root[0].as_int(), root[1].as_int(), root[2].as_int(), root[3].as_int());
}

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
    let mut fresh_store = false;

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
            "--fresh" => {
                fresh_store = true;
                i += 1;
            }
            "--help" | "-h" => {
                println!("Usage: verify-notes [OPTIONS]");
                println!();
                println!("Options:");
                println!("  --note-id, -n <ID>  Query a specific note from the node by its ID");
                println!("  --fresh             Clear local store before querying (fresh start)");
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

    // Clear store if --fresh flag is set
    if fresh_store && store_path.exists() {
        println!("Clearing local store (--fresh)...");
        std::fs::remove_dir_all(&store_path)?;
    }

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

        // Parse note ID (try_from_hex expects 0x prefix)
        let note_id_hex = if note_id_str.starts_with("0x") {
            note_id_str.clone()
        } else {
            format!("0x{}", note_id_str)
        };
        let note_id = NoteId::try_from_hex(&note_id_hex)
            .map_err(|e| format!("Invalid note ID '{}': {:?}", note_id_str, e))?;

        // First check if note is already in local store
        let input_notes = client.get_input_notes(NoteFilter::All).await
            .map_err(|e| format!("Failed to get input notes: {}", e))?;

        if let Some(note) = input_notes.iter().find(|n| n.id() == note_id) {
            println!("✓ Note found in local store!");
            println!();
            print_note_details(note, "  ");
            println!();
        } else {
            // Try to import/fetch the note from the node
            println!("Note not in local store, querying node...");
            let note_files = vec![NoteFile::NoteId(note_id)];
            match client.import_notes(&note_files).await {
                Ok(imported_ids) => {
                    if imported_ids.is_empty() {
                        println!("✗ Note not found on node");
                        println!();
                        println!("Note: The node only returns public notes. Private notes");
                        println!("require the full note details to be imported.");
                    } else {
                        println!("✓ Note found and imported from node!");
                        println!();

                        // Refresh the list after import
                        let input_notes = client.get_input_notes(NoteFilter::All).await
                            .map_err(|e| format!("Failed to get input notes: {}", e))?;

                        for imported_id in &imported_ids {
                            if let Some(note) = input_notes.iter().find(|n| n.id() == *imported_id) {
                                print_note_details(note, "  ");
                            } else {
                                println!("  Note ID: {}", format_note_id(imported_id));
                                println!("  (details not available)");
                            }
                            println!();
                        }
                    }
                }
                Err(e) => {
                    println!("✗ Error fetching note:");
                    println!("  {}", e);
                    println!();
                    println!("Note: The node only returns public notes or notes you have");
                    println!("the proper authentication to access.");
                }
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
            print_note_details(note, "    ");
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
            println!("    ID:     {}", format_note_id(&note.id()));
            // Output notes have different state representation
            let state = format!("{:?}", note.state());
            let state_name = state.split('(').next().unwrap_or(&state);
            println!("    State:  {}", state_name);
            let assets = note.assets();
            if assets.num_assets() > 0 {
                println!("    Assets: {} asset(s)", assets.num_assets());
                for asset in assets.iter() {
                    println!("      - {}", format_asset(&asset));
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
