//! Note verification tool for Miden node
//!
//! Queries the miden node to list and verify available notes.
//! Replaces the shell-only verify-notes.sh with a proper Rust implementation.

use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::rpc::Endpoint;
use miden_client::Client;
use miden_client_sqlite_store::SqliteStore;
use miden_protocol::account::AccountId;
use miden_protocol::utils::Serializable;

type VerifyClient = Client<FilesystemKeyStore>;

#[derive(Parser)]
#[command(name = "verify-notes")]
#[command(about = "Query and verify notes on the Miden node")]
#[command(version)]
struct Cli {
    /// Miden node RPC endpoint
    #[arg(long, env = "MIDEN_RPC_URL", default_value = "http://localhost:57291")]
    rpc_url: String,

    /// Path to store directory (defaults to ~/.miden-verify)
    #[arg(long, env = "MIDEN_STORE_PATH")]
    store_path: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Sync with the Miden node
    Sync,
    /// List notes (default command)
    List {
        /// Filter by note status: all, consumable
        #[arg(long, default_value = "consumable")]
        filter: String,
        /// Account ID to filter notes for (hex string)
        #[arg(long)]
        account: Option<String>,
    },
    /// Show client info and sync status
    Info,
    /// Show details for a specific note
    Show {
        /// Note ID (hex string or prefix)
        note_id: String,
    },
    /// List all accounts tracked by the client
    Accounts,
}

fn get_default_store_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".miden-verify")
}

async fn create_client(rpc_url: &str, store_path: PathBuf) -> Result<VerifyClient> {
    // Ensure store directory exists
    std::fs::create_dir_all(&store_path)
        .with_context(|| format!("Failed to create store directory: {:?}", store_path))?;

    let db_path = store_path.join("store.db");
    let keystore_path = store_path.join("keystore");

    std::fs::create_dir_all(&keystore_path)
        .with_context(|| format!("Failed to create keystore directory: {:?}", keystore_path))?;

    // Create SQLite store
    let store = SqliteStore::new(db_path)
        .await
        .context("Failed to create SQLite store")?;

    // Parse RPC endpoint
    let endpoint = Endpoint::try_from(rpc_url)
        .map_err(|e| anyhow!("Invalid RPC endpoint: {} - {}", rpc_url, e))?;

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

fn parse_account_id(s: &str) -> Result<AccountId> {
    // Remove 0x prefix if present
    let s = s.strip_prefix("0x").unwrap_or(s);

    // Try parsing as hex-encoded AccountId
    AccountId::from_hex(s)
        .map_err(|e| anyhow!("Invalid account ID '{}': {}", s, e))
}

async fn cmd_sync(client: &mut VerifyClient) -> Result<()> {
    println!("Syncing with Miden node...");

    let sync_result = client.sync_state().await
        .context("Failed to sync state")?;

    println!("Sync complete!");
    println!("  Block number: {}", sync_result.block_num);
    println!("  New public notes: {}", sync_result.new_public_notes.len());
    println!("  Consumed notes: {}", sync_result.consumed_notes.len());
    println!("  Updated accounts: {}", sync_result.updated_accounts.len());

    Ok(())
}

async fn cmd_info(client: &mut VerifyClient) -> Result<()> {
    let height = client.get_sync_height().await
        .context("Failed to get sync height")?;

    let accounts = client.get_account_headers().await
        .context("Failed to get account headers")?;

    println!("Client Info:");
    println!("  Sync height: {}", height);
    println!("  Tracked accounts: {}", accounts.len());

    Ok(())
}

async fn cmd_list(client: &mut VerifyClient, filter: &str, account: Option<String>) -> Result<()> {
    // Parse account ID if provided
    let account_id = match account {
        Some(ref s) => Some(parse_account_id(s)?),
        None => None,
    };

    // First sync to get latest state
    println!("Syncing...");
    let _ = client.sync_state().await;

    match filter {
        "consumable" | "all" => {
            let notes = client.get_consumable_notes(account_id).await
                .context("Failed to get consumable notes")?;

            if notes.is_empty() {
                println!("\nNo consumable notes found.");
            } else {
                println!("\nConsumable Notes ({}):", notes.len());
                println!("{}", "=".repeat(80));

                for (note_record, _status) in &notes {
                    let note_id = note_record.id();

                    println!("Note ID: 0x{}", hex::encode(note_id.to_bytes()));

                    // Try to get assets if available
                    let assets = note_record.assets();
                    if assets.num_assets() > 0 {
                        println!("  Assets ({}):", assets.num_assets());
                        for asset in assets.iter() {
                            match asset {
                                miden_protocol::asset::Asset::Fungible(fa) => {
                                    println!("    - Fungible: {} units from faucet {}",
                                        fa.amount(),
                                        fa.faucet_id());
                                }
                                miden_protocol::asset::Asset::NonFungible(nfa) => {
                                    println!("    - Non-fungible: {:?}", nfa);
                                }
                            }
                        }
                    } else {
                        println!("  Assets: none");
                    }

                    // Show metadata if available
                    if let Some(metadata) = note_record.metadata() {
                        println!("  Sender: {}", metadata.sender());
                        println!("  Tag: {}", metadata.tag());
                    }

                    println!("{}", "-".repeat(80));
                }
            }
        }
        other => {
            println!("Filter '{}' not yet implemented. Available: consumable, all", other);
            println!("Note: The Rust client API primarily exposes consumable notes.");
        }
    }

    Ok(())
}

async fn cmd_show(client: &mut VerifyClient, note_id: &str) -> Result<()> {
    // Sync first
    let _ = client.sync_state().await;

    // Get all consumable notes and find the one matching
    let notes = client.get_consumable_notes(None).await
        .context("Failed to get notes")?;

    let note_id_clean = note_id.strip_prefix("0x").unwrap_or(note_id).to_lowercase();

    for (note_record, _status) in &notes {
        let id_hex = hex::encode(note_record.id().to_bytes()).to_lowercase();

        if id_hex.starts_with(&note_id_clean) || note_id_clean.starts_with(&id_hex) {
            println!("Note Details:");
            println!("{}", "=".repeat(80));
            println!("ID: 0x{}", id_hex);

            // Show metadata if available
            if let Some(metadata) = note_record.metadata() {
                println!("\nMetadata:");
                println!("  Sender: {}", metadata.sender());
                println!("  Tag: {}", metadata.tag());
            }

            // Assets
            let assets = note_record.assets();
            println!("\nAssets ({}):", assets.num_assets());
            for asset in assets.iter() {
                match asset {
                    miden_protocol::asset::Asset::Fungible(fa) => {
                        println!("  - Fungible: {} units", fa.amount());
                        println!("    Faucet: {}", fa.faucet_id());
                    }
                    miden_protocol::asset::Asset::NonFungible(nfa) => {
                        println!("  - Non-fungible: {:?}", nfa);
                    }
                }
            }

            return Ok(());
        }
    }

    println!("Note not found: {}", note_id);
    println!("Note: Only consumable notes can be displayed with full details.");

    Ok(())
}

async fn cmd_accounts(client: &mut VerifyClient) -> Result<()> {
    let accounts = client.get_account_headers().await
        .context("Failed to get account headers")?;

    if accounts.is_empty() {
        println!("No accounts tracked by this client.");
        println!("Hint: Import an account or create one to start tracking notes.");
    } else {
        println!("Tracked Accounts ({}):", accounts.len());
        println!("{}", "=".repeat(80));

        for (header, _seed) in &accounts {
            println!("Account ID: {}", header.id());
            println!("  Nonce: {}", header.nonce());
            println!("  Vault root: 0x{}", hex::encode(header.vault_root().as_bytes()));
            println!("{}", "-".repeat(80));
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing if RUST_LOG is set
    if env::var("RUST_LOG").is_ok() {
        tracing_subscriber::fmt::init();
    }

    let cli = Cli::parse();

    let store_path = cli.store_path.unwrap_or_else(get_default_store_path);

    println!("Miden Note Verification Tool");
    println!("RPC endpoint: {}", cli.rpc_url);
    println!("Store path: {:?}", store_path);
    println!();

    let mut client = create_client(&cli.rpc_url, store_path).await?;

    match cli.command {
        None | Some(Commands::List { filter: _, account: _ }) => {
            // Default to list consumable
            let (filter, account) = match &cli.command {
                Some(Commands::List { filter, account }) => (filter.as_str(), account.clone()),
                _ => ("consumable", None),
            };
            cmd_list(&mut client, filter, account).await?;
        }
        Some(Commands::Sync) => {
            cmd_sync(&mut client).await?;
        }
        Some(Commands::Info) => {
            cmd_info(&mut client).await?;
        }
        Some(Commands::Show { note_id }) => {
            cmd_show(&mut client, &note_id).await?;
        }
        Some(Commands::Accounts) => {
            cmd_accounts(&mut client).await?;
        }
    }

    Ok(())
}
