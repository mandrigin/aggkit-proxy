#!/bin/bash
# Patch miden-node to support agglayer faucets in genesis config
# This script is applied during Docker build after cloning miden-node

set -e

echo "=== Applying agglayer faucet genesis patch ==="

# 1. Add miden-agglayer to workspace dependencies
echo "Adding miden-agglayer to workspace Cargo.toml..."
if ! grep -q "miden-agglayer" Cargo.toml; then
    # Add after miden-tx-batch-prover line
    sed -i '/miden-tx-batch-prover.*agglayer-v0.1/a\
miden-agglayer        = { tag = "agglayer-v0.1", git = "https://github.com/0xMiden/miden-base.git" }' Cargo.toml
fi

# 2. Add miden-agglayer dependency to store crate
echo "Adding miden-agglayer to store crate..."
if ! grep -q "miden-agglayer" crates/store/Cargo.toml; then
    sed -i '/miden-standards.*workspace/a\
miden-agglayer         = { workspace = true }' crates/store/Cargo.toml
fi

# 3. Add imports to genesis config module
echo "Adding imports to genesis config..."
GENESIS_FILE="crates/store/src/genesis/config/mod.rs"

if ! grep -q "miden_agglayer" "$GENESIS_FILE"; then
    # Add import after miden_node_utils import
    sed -i '/use miden_node_utils::crypto::get_rpo_random_coin;/a\
use miden_agglayer::{create_agglayer_faucet_component, create_bridge_account};' "$GENESIS_FILE"
fi

# 4. Add agglayer_faucet field to GenesisConfig struct
echo "Adding agglayer_faucet field to GenesisConfig..."
if ! grep -q "agglayer_faucet:" "$GENESIS_FILE"; then
    sed -i '/fungible_faucet: Vec<FungibleFaucetConfig>,/a\
    #[serde(default)]\
    agglayer_faucet: Vec<AgglayerFaucetConfig>,' "$GENESIS_FILE"
fi

# 5. Add agglayer_faucet to Default impl
echo "Adding agglayer_faucet to Default impl..."
if ! grep -q "agglayer_faucet: vec!\[\]" "$GENESIS_FILE"; then
    sed -i '/fungible_faucet: vec!\[\],/a\
            agglayer_faucet: vec![],' "$GENESIS_FILE"
fi

# 6. Add agglayer_faucet to destructuring in into_state
echo "Adding agglayer_faucet to into_state destructuring..."
if ! grep -q "agglayer_faucet: agglayer_faucet_configs" "$GENESIS_FILE"; then
    sed -i '/fungible_faucet: fungible_faucet_configs,/a\
            agglayer_faucet: agglayer_faucet_configs,' "$GENESIS_FILE"
fi

# 7. Add AgglayerFaucetConfig struct and processing logic
echo "Adding AgglayerFaucetConfig struct..."
if ! grep -q "struct AgglayerFaucetConfig" "$GENESIS_FILE"; then
    cat >> "$GENESIS_FILE" << 'AGGLAYER_EOF'

// AGGLAYER FAUCET CONFIG (added by patch for bridge claim support)
// ================================================================================================

/// Represents an agglayer faucet with bridge account support for CLAIM notes.
/// This faucet type can process bridge claims from L1.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgglayerFaucetConfig {
    /// Token symbol (e.g., "LUMIA")
    symbol: TokenSymbolStr,
    /// Number of decimal places
    decimals: u8,
    /// Max supply in smallest units
    max_supply: u64,
    /// Storage mode for the faucet account
    #[serde(default)]
    storage_mode: StorageMode,
}

impl AgglayerFaucetConfig {
    /// Create an agglayer faucet with bridge account from config.
    /// Returns (faucet_account, bridge_account, secret_key).
    fn build_account(self) -> Result<(Account, Account, miden_protocol::crypto::dsa::falcon512_rpo::SecretKey), GenesisConfigError> {
        use miden_protocol::crypto::dsa::falcon512_rpo::SecretKey as RpoSecretKey;

        let AgglayerFaucetConfig { symbol, decimals, max_supply, storage_mode } = self;

        let mut rng = ChaCha20Rng::from_seed(rand::random());
        let secret_key = RpoSecretKey::with_rng(&mut get_rpo_random_coin(&mut rng));
        let auth = AuthRpoFalcon512::new(secret_key.public_key().into());

        // Create bridge account first (needed for agglayer faucet)
        let bridge_seed: [u8; 32] = rng.random();
        let bridge_word = miden_protocol::Word::new([
            Felt::new(u64::from_le_bytes(bridge_seed[0..8].try_into().unwrap())),
            Felt::new(u64::from_le_bytes(bridge_seed[8..16].try_into().unwrap())),
            Felt::new(u64::from_le_bytes(bridge_seed[16..24].try_into().unwrap())),
            Felt::new(u64::from_le_bytes(bridge_seed[24..32].try_into().unwrap())),
        ]);
        let bridge_account = create_bridge_account(bridge_word);

        let faucet_seed: [u8; 32] = rng.random();
        let max_supply_felt = Felt::try_from(max_supply).expect("max_supply fits in Felt");

        // Create agglayer faucet component with bridge account reference
        let agglayer_component = create_agglayer_faucet_component(
            &symbol.raw,
            decimals,
            max_supply_felt,
            bridge_account.id(),
        );

        // Build the agglayer faucet account
        let faucet_account = AccountBuilder::new(faucet_seed)
            .account_type(AccountType::FungibleFaucet)
            .storage_mode(storage_mode.into())
            .with_auth_component(auth)
            .with_component(agglayer_component)
            .build()?;

        Ok((faucet_account, bridge_account, secret_key))
    }
}
AGGLAYER_EOF
fi

# 8. Add processing loop for agglayer faucets in into_state function
# This adds code AFTER the wallet processing loop ends (after "// Wallets")
echo "Adding agglayer faucet processing to into_state..."
if ! grep -q "Setup agglayer faucets" "$GENESIS_FILE"; then
    # Add processing after "// Wallets" section which is after faucet loop
    sed -i '/\/\/ Wallets/i\
        // Setup agglayer faucets with bridge account support\
        for agglayer_config in agglayer_faucet_configs {\
            let symbol = agglayer_config.symbol.clone();\
            let (faucet_account, bridge_account, secret_key) = agglayer_config.build_account()?;\
\
            if faucet_accounts.insert(symbol.clone(), faucet_account.clone()).is_some() {\
                return Err(GenesisConfigError::DuplicateFaucetDefinition { symbol });\
            }\
\
            secrets.push((\
                format!("agglayer_faucet_{symbol}.mac", symbol = symbol.to_string().to_lowercase()),\
                faucet_account.id(),\
                secret_key.clone(),\
            ));\
\
            // Bridge account is stored separately (no assets, just for validation)\
            secrets.push((\
                format!("bridge_{symbol}.mac", symbol = symbol.to_string().to_lowercase()),\
                bridge_account.id(),\
                secret_key,\
            ));\
\
            // Add bridge account to all_accounts list\
            wallet_accounts.push(bridge_account);\
        }\
' "$GENESIS_FILE"
fi

echo "=== Agglayer faucet genesis patch complete ==="
echo ""
echo "The following changes were made:"
echo "  1. Added miden-agglayer to workspace dependencies"
echo "  2. Added miden-agglayer to store crate dependencies"
echo "  3. Added AgglayerFaucetConfig struct"
echo "  4. Added agglayer_faucet field to GenesisConfig"
echo "  5. Added processing loop for agglayer faucets"
echo ""
echo "You can now use [[agglayer_faucet]] in genesis.toml"
