//! Note Verification Tool
//!
//! A Rust binary to query and verify notes available on the Miden node.
//!
//! Usage:
//!   verify-notes                         # List all notes
//!   verify-notes --filter consumable     # List only consumable notes
//!   verify-notes --sync-only             # Just sync without listing
//!   verify-notes --summary               # Show summary of all note types

use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, ValueEnum};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::rpc::Endpoint;
use miden_client::Client;
use miden_client_sqlite_store::SqliteStore;
use tracing::info;
use tracing_subscriber::EnvFilter;

/// Note filter types matching miden-client's note status categories
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
enum NoteFilter {
    /// All notes (default)
    #[default]
    All,
    /// Notes that can be consumed
    Consumable,
    /// Notes that have been committed to the chain
    Committed,
    /// Notes that have been consumed
    Consumed,
    /// Notes that are being processed
    Processing,
    /// Expected notes (not yet on chain)
    Expected,
}

/// Miden Note Verification Tool
///
/// Query and verify notes available on the Miden node.
#[derive(Parser, Debug)]
#[command(name = "verify-notes")]
#[command(about = "Query and verify notes on the Miden node")]
#[command(version)]
struct Args {
    /// Miden node RPC endpoint
    #[arg(long, env = "MIDEN_RPC_URL", default_value = "http://localhost:57291")]
    rpc_url: String,

    /// Path to store client state (SQLite database)
    #[arg(long, env = "MIDEN_STORE_PATH")]
    store_path: Option<PathBuf>,

    /// Filter notes by status
    #[arg(long, short, value_enum, default_value = "all")]
    filter: NoteFilter,

    /// Only sync with the node, don't list notes
    #[arg(long)]
    sync_only: bool,

    /// Show summary counts of all note types
    #[arg(long)]
    summary: bool,

    /// Show client info
    #[arg(long)]
    info: bool,

    /// Account ID to filter notes for (optional, hex format)
    #[arg(long)]
    account: Option<String>,
}

type VerifyClient = Client<FilesystemKeyStore>;

/// Get default store path in user's home directory
fn default_store_path() -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".miden-verify-notes")
}

/// Initialize the Miden client
async fn init_client(rpc_url: &str, store_path: &PathBuf) -> Result<VerifyClient> {
    // Create directories if needed
    std::fs::create_dir_all(store_path)
        .with_context(|| format!("Failed to create store directory: {:?}", store_path))?;

    let db_path = store_path.join("store.db");
    let keystore_path = store_path.join("keystore");
    std::fs::create_dir_all(&keystore_path)?;

    // Initialize SQLite store
    let store = SqliteStore::new(db_path)
        .await
        .context("Failed to initialize SQLite store")?;

    // Parse RPC endpoint
    let endpoint = Endpoint::try_from(rpc_url)
        .map_err(|e| anyhow!("Invalid RPC endpoint '{}': {}", rpc_url, e))?;

    let keystore_path_str = keystore_path.to_string_lossy().to_string();

    // Build client
    let client: VerifyClient = ClientBuilder::new()
        .grpc_client(&endpoint, Some(10_000))
        .store(Arc::new(store))
        .filesystem_keystore(&keystore_path_str)
        .build()
        .await
        .context("Failed to build Miden client")?;

    Ok(client)
}

/// Sync client state with the node
async fn sync_client(client: &mut VerifyClient) -> Result<()> {
    println!("Syncing with Miden node...");

    let sync_result = client
        .sync_state()
        .await
        .context("Failed to sync state with node")?;

    println!(
        "✓ Synced to block {} ({} new notes, {} consumed)",
        sync_result.block_num.as_u32(),
        sync_result.new_public_notes.len(),
        sync_result.consumed_notes.len()
    );

    Ok(())
}

/// Show client info
async fn show_info(client: &VerifyClient) -> Result<()> {
    println!("\n=== Client Info ===");

    let height = client
        .get_sync_height()
        .await
        .context("Failed to get sync height")?;
    println!("Sync height: {}", height.as_u32());

    let accounts = client
        .get_account_headers()
        .await
        .context("Failed to get account headers")?;
    println!("Known accounts: {}", accounts.len());

    for (header, _) in &accounts {
        println!("  - {}", header.id());
    }

    Ok(())
}

/// Parse an account ID from hex string
fn parse_account_id(s: &str) -> Result<miden_protocol::account::AccountId> {
    miden_protocol::account::AccountId::from_hex(s)
        .map_err(|e| anyhow!("Invalid account ID '{}': {:?}", s, e))
}

/// List notes with the given filter
async fn list_notes(client: &VerifyClient, filter: NoteFilter, account_id: Option<&str>) -> Result<()> {
    println!("\n=== Notes (filter: {:?}) ===\n", filter);

    // Parse account ID if provided
    let account = if let Some(id_str) = account_id {
        let id = parse_account_id(id_str)?;
        Some(id)
    } else {
        None
    };

    match filter {
        NoteFilter::All | NoteFilter::Consumable => {
            let notes = client
                .get_consumable_notes(account)
                .await
                .context("Failed to get consumable notes")?;

            if notes.is_empty() {
                println!("No consumable notes found.");
            } else {
                println!("Found {} consumable note(s):\n", notes.len());
                for (note, relevance) in &notes {
                    // Print note summary
                    let assets: Vec<String> = note
                        .assets()
                        .iter()
                        .map(|a| format!("{:?}", a))
                        .collect();

                    let tag_str = match note.metadata() {
                        Some(metadata) => format!("{}", metadata.tag()),
                        None => "(none)".to_string(),
                    };

                    println!(
                        "  {} | assets: {} | tag: {} | relevance: {:?}",
                        note.id(),
                        if assets.is_empty() {
                            "none".to_string()
                        } else {
                            assets.join(", ")
                        },
                        tag_str,
                        relevance
                    );
                }
            }
        }
        _ => {
            // For other filters, we can only get consumable notes via the client API
            println!("Note: Filter '{:?}' requires direct store access.", filter);
            println!("Use --filter consumable or all for available notes.");
        }
    }

    Ok(())
}

/// Show summary of note counts
async fn show_summary(client: &VerifyClient) -> Result<()> {
    println!("\n=== Note Summary ===\n");

    let consumable = client
        .get_consumable_notes(None)
        .await
        .context("Failed to get consumable notes")?;

    println!("Consumable notes: {}", consumable.len());

    // Group by tag if there are any notes
    if !consumable.is_empty() {
        use std::collections::HashMap;
        let mut by_tag: HashMap<String, usize> = HashMap::new();

        for (note, _) in &consumable {
            let key = match note.metadata() {
                Some(metadata) => format!("{}", metadata.tag()),
                None => "(no metadata)".to_string(),
            };
            *by_tag.entry(key).or_insert(0) += 1;
        }

        if by_tag.len() > 1 {
            println!("\nBy tag:");
            for (tag, count) in by_tag {
                println!("  {}: {}", tag, count);
            }
        }

        // Show total assets
        let mut total_assets = 0usize;
        for (note, _) in &consumable {
            total_assets += note.assets().iter().count();
        }
        println!("\nTotal assets across all notes: {}", total_assets);
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    let args = Args::parse();

    println!("========================================");
    println!("  Miden Node Note Verification Tool");
    println!("========================================\n");

    let store_path = args.store_path.unwrap_or_else(default_store_path);

    println!("RPC endpoint: {}", args.rpc_url);
    println!("Store path: {:?}", store_path);

    // Initialize client
    info!("Initializing Miden client...");
    let mut client = init_client(&args.rpc_url, &store_path)
        .await
        .context("Failed to initialize client")?;
    println!("✓ Client initialized\n");

    // Show info if requested
    if args.info {
        show_info(&client).await?;
    }

    // Sync with node
    sync_client(&mut client).await?;

    // Exit early if sync-only
    if args.sync_only {
        println!("\n✓ Sync complete.");
        return Ok(());
    }

    // Show summary
    if args.summary {
        show_summary(&client).await?;
        return Ok(());
    }

    // List notes (default action)
    list_notes(&client, args.filter, args.account.as_deref()).await?;

    Ok(())
}
