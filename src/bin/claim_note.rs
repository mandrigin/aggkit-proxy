//! Claim Note - Consume a Miden note by ID using a deterministic account
//!
//! This binary derives a Miden account from a seed phrase and claims notes to it.
//!
//! Usage:
//!   claim-note derive-address              # Print the derived account address
//!   claim-note claim <note-id>             # Claim a note to the derived account
//!
//! Environment variables:
//!   CLAIMER_SEED - Seed phrase for deterministic account derivation (required)
//!   MIDEN_RPC_URL - Node RPC endpoint (default: http://localhost:57291)
//!   MIDEN_STORE_PATH - Client store path (default: /tmp/miden-claimer)

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::component::{AccountComponent, BasicWallet};
use miden_standards::account::auth::AuthFalcon512Rpo;
use miden_client::account::{AccountBuilder, AccountType};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::notes::NoteFile;
use miden_client::rpc::Endpoint;
use miden_client::store::NoteFilter;
use miden_client::transaction::TransactionRequestBuilder;
use miden_client::Client;
use miden_client_sqlite_store::SqliteStore;
use miden_protocol::account::auth::AuthSecretKey;
use miden_protocol::account::{AccountId, AccountStorageMode};
use miden_protocol::note::NoteId;
use rand_chacha::ChaCha20Rng;
use rand::SeedableRng;
use sha3::{Digest, Keccak256};
use tracing_subscriber::EnvFilter;

/// Derive a 32-byte seed from a seed phrase using Keccak256
fn derive_seed(phrase: &str) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(b"miden-claimer-seed-v1:");
    hasher.update(phrase.as_bytes());
    hasher.finalize().into()
}

/// Derive a deterministic Falcon512 key pair from a seed
fn derive_key_pair(seed: &[u8; 32]) -> AuthSecretKey {
    // Derive a sub-seed for the key
    let mut hasher = Keccak256::new();
    hasher.update(b"miden-claimer-key-v1:");
    hasher.update(seed);
    let key_seed: [u8; 32] = hasher.finalize().into();

    // Create a seeded RNG
    let mut rng = ChaCha20Rng::from_seed(key_seed);

    // Generate the key pair deterministically
    AuthSecretKey::new_falcon512_rpo_with_rng(&mut rng)
}

/// Derive a deterministic init seed for account creation
fn derive_init_seed(seed: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(b"miden-claimer-init-v1:");
    hasher.update(seed);
    hasher.finalize().into()
}

/// Create or get the deterministic account
async fn get_or_create_account(
    client: &mut Client<FilesystemKeyStore>,
    keystore: &FilesystemKeyStore,
    seed: &[u8; 32],
) -> Result<AccountId, Box<dyn std::error::Error>> {
    // Derive key pair and init seed
    let key_pair = derive_key_pair(seed);
    let init_seed = derive_init_seed(seed);

    // Create auth component from the public key
    let auth_component: AccountComponent =
        AuthFalcon512Rpo::new(key_pair.public_key().to_commitment()).into();

    // Build account
    let account = AccountBuilder::new(init_seed)
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(auth_component)
        .with_component(BasicWallet)
        .build()?;

    let account_id = account.id();

    // Check if account already exists in client
    let existing = client.get_account(account_id).await;
    if let Ok(Some(_)) = existing {
        return Ok(account_id);
    }

    // Add key to keystore (ignore error if already exists)
    let _ = keystore.add_key(&key_pair);

    // Add account to client
    client.add_account(&account, false).await?;

    Ok(account_id)
}

fn print_usage() {
    eprintln!("Usage: claim-note <command> [args]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  derive-address    Print the derived account address (no network required)");
    eprintln!("  claim <note-id>   Claim a note to the derived account");
    eprintln!();
    eprintln!("Environment variables:");
    eprintln!("  CLAIMER_SEED     - Seed phrase for account derivation (required)");
    eprintln!("  MIDEN_RPC_URL    - Node RPC endpoint (default: http://localhost:57291)");
    eprintln!("  MIDEN_STORE_PATH - Client store path (default: /tmp/miden-claimer)");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    // Parse command-line arguments
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    let command = &args[1];

    // Get seed phrase from environment
    let seed_phrase = std::env::var("CLAIMER_SEED")
        .map_err(|_| "CLAIMER_SEED environment variable not set")?;

    let seed = derive_seed(&seed_phrase);

    match command.as_str() {
        "derive-address" | "address" | "addr" => {
            // Just derive and print the address - no network needed
            let key_pair = derive_key_pair(&seed);
            let init_seed = derive_init_seed(&seed);

            let auth_component: AccountComponent =
                AuthFalcon512Rpo::new(key_pair.public_key().to_commitment()).into();

            let account = AccountBuilder::new(init_seed)
                .account_type(AccountType::RegularAccountUpdatableCode)
                .storage_mode(AccountStorageMode::Public)
                .with_auth_component(auth_component)
                .with_component(BasicWallet)
                .build()?;

            let account_id = account.id();
            let miden_hex = format!("0x{}", hex::encode(<[u8; 15]>::from(account_id)));

            // Also compute the Eth-padded version
            let mut eth_bytes = [0u8; 20];
            eth_bytes[5..].copy_from_slice(&<[u8; 15]>::from(account_id));
            let eth_hex = format!("0x{}", hex::encode(eth_bytes));

            println!("Derived Claimer Account");
            println!("  Miden: {}", miden_hex);
            println!("  Eth:   {}", eth_hex);
            println!();
            println!("Use for deposits:");
            println!("  ./scripts/send-deposit.sh <amount> {}", miden_hex);

            return Ok(());
        }

        "claim" => {
            if args.len() < 3 {
                eprintln!("Usage: claim-note claim <note-id>");
                std::process::exit(1);
            }

            let note_id_str = &args[2];

            let rpc_url = std::env::var("MIDEN_RPC_URL")
                .unwrap_or_else(|_| "http://localhost:57291".to_string());

            let store_path = std::env::var("MIDEN_STORE_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/tmp/miden-claimer"));

            // Parse note ID
            let note_id_hex = if note_id_str.starts_with("0x") {
                note_id_str.clone()
            } else {
                format!("0x{}", note_id_str)
            };
            let note_id = NoteId::try_from_hex(&note_id_hex)
                .map_err(|e| format!("Invalid note ID '{}': {:?}", note_id_str, e))?;

            println!("╔══════════════════════════════════════════════════════════════════╗");
            println!("║                    Miden Note Claimer                             ║");
            println!("╚══════════════════════════════════════════════════════════════════╝");
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

            let keystore = FilesystemKeyStore::new(keystore_path.clone())
                .map_err(|e| format!("Failed to create keystore: {}", e))?;
            let keystore_arc = Arc::new(keystore);

            let mut client: Client<FilesystemKeyStore> = ClientBuilder::new()
                .grpc_client(&endpoint, Some(10_000))
                .store(Arc::new(store))
                .authenticator(keystore_arc.clone())
                .build()
                .await
                .map_err(|e| format!("Failed to build client: {}", e))?;

            println!("✓ Client initialized");

            // Get or create the deterministic account
            println!("Setting up claimer account...");
            let keystore_for_account = FilesystemKeyStore::new(keystore_path.clone())
                .map_err(|e| format!("Failed to open keystore: {}", e))?;
            let claimer_account_id = get_or_create_account(&mut client, &keystore_for_account, &seed).await?;

            let claimer_hex = format!("0x{}", hex::encode(<[u8; 15]>::from(claimer_account_id)));
            println!("✓ Claimer account: {}", claimer_hex);
            println!();

            println!("Configuration:");
            println!("  RPC URL:    {}", rpc_url);
            println!("  Store Path: {}", store_path.display());
            println!("  Note ID:    {}", note_id_hex);
            println!("  Claimer:    {}", claimer_hex);
            println!();

            // Sync state
            println!("Syncing state with node...");
            let sync_result = client
                .sync_state()
                .await
                .map_err(|e| format!("Sync failed: {}", e))?;

            println!("✓ Synced to block {}", sync_result.block_num.as_u32());
            println!();

            // Check if note is already in local store
            println!("Looking for note...");
            let input_notes = client
                .get_input_notes(NoteFilter::All)
                .await
                .map_err(|e| format!("Failed to get input notes: {}", e))?;

            let note_in_store = input_notes.iter().find(|n| n.id() == note_id);

            if note_in_store.is_none() {
                println!("  Note not in local store, importing from node...");
                let note_files = vec![NoteFile::NoteId(note_id)];
                match client.import_notes(&note_files).await {
                    Ok(imported) if !imported.is_empty() => {
                        println!("  ✓ Note imported from node");
                    }
                    Ok(_) => {
                        eprintln!("✗ Note not found on node");
                        eprintln!();
                        eprintln!("The note may be private or not yet committed.");
                        std::process::exit(1);
                    }
                    Err(e) => {
                        eprintln!("✗ Failed to import note: {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                println!("  ✓ Note found in local store");
            }

            // Get consumable notes for this account
            println!();
            println!("Checking if note is consumable by account...");
            let consumable = client
                .get_consumable_notes(Some(claimer_account_id))
                .await
                .map_err(|e| format!("Failed to get consumable notes: {}", e))?;

            let note_record = consumable
                .into_iter()
                .find(|(n, _)| n.id() == note_id)
                .map(|(n, _)| n);

            let note: miden_protocol::note::Note = match note_record {
                Some(record) => record
                    .try_into()
                    .map_err(|e| format!("Failed to convert note: {:?}", e))?,
                None => {
                    eprintln!(
                        "✗ Note {} is not consumable by account {}",
                        note_id_hex, claimer_hex
                    );
                    eprintln!();
                    eprintln!("Possible reasons:");
                    eprintln!("  - Note is not addressed to this account");
                    eprintln!("  - Note has already been consumed");
                    eprintln!("  - Account doesn't exist or isn't tracked by client");
                    std::process::exit(1);
                }
            };

            println!("  ✓ Note is consumable");
            println!();

            // Build and submit consume transaction
            println!("Building consume transaction...");
            let tx_request = TransactionRequestBuilder::new()
                .build_consume_notes(vec![note])
                .map_err(|e| format!("Failed to build consume request: {}", e))?;

            println!("Submitting transaction...");
            let tx_id = client
                .submit_new_transaction(claimer_account_id, tx_request)
                .await
                .map_err(|e| format!("Failed to submit transaction: {}", e))?;

            println!();
            println!("═══════════════════════════════════════════════════════════════════");
            println!("                         SUCCESS                                    ");
            println!("═══════════════════════════════════════════════════════════════════");
            println!();
            println!("  Note claimed successfully!");
            println!("  Transaction ID: {}", tx_id);
            println!();
        }

        _ => {
            eprintln!("Unknown command: {}", command);
            print_usage();
            std::process::exit(1);
        }
    }

    Ok(())
}
