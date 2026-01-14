//! Configuration for the Miden RPC proxy.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Proxy server configuration.
///
/// Loaded from TOML configuration file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Port to listen on for JSON-RPC requests.
    #[serde(default = "default_listen_port")]
    pub listen_port: u16,

    /// Miden node RPC endpoint URL.
    pub miden_rpc_url: String,

    /// Chain ID to return for eth_chainId requests.
    pub chain_id: u64,

    /// Miden account ID of the bridge account.
    ///
    /// This is the account that holds the bridged assets and creates
    /// P2ID notes for claim recipients.
    pub bridge_account_id: String,
}

fn default_listen_port() -> u16 {
    8545
}

impl ProxyConfig {
    /// Load configuration from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path.as_ref()).map_err(|e| ConfigError::Io {
            path: path.as_ref().display().to_string(),
            source: e,
        })?;

        toml::from_str(&content).map_err(|e| ConfigError::Parse {
            path: path.as_ref().display().to_string(),
            source: e,
        })
    }

    /// Load configuration from a TOML string.
    ///
    /// # Errors
    ///
    /// Returns an error if the string cannot be parsed.
    pub fn from_str(content: &str) -> Result<Self, ConfigError> {
        toml::from_str(content).map_err(|e| ConfigError::Parse {
            path: "<string>".to_string(),
            source: e,
        })
    }
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            listen_port: default_listen_port(),
            miden_rpc_url: "http://localhost:57291".to_string(),
            chain_id: 1,
            bridge_account_id: String::new(),
        }
    }
}

/// Configuration loading errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Failed to read configuration file.
    #[error("failed to read config file '{path}': {source}")]
    Io {
        /// Path to the config file.
        path: String,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to parse configuration file.
    #[error("failed to parse config file '{path}': {source}")]
    Parse {
        /// Path to the config file.
        path: String,
        /// Underlying parse error.
        #[source]
        source: toml::de::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config_from_string() {
        let config_str = r#"
            listen_port = 8546
            miden_rpc_url = "http://miden-node:57291"
            chain_id = 42
            bridge_account_id = "0x1234567890abcdef"
        "#;

        let config = ProxyConfig::from_str(config_str).unwrap();
        assert_eq!(config.listen_port, 8546);
        assert_eq!(config.miden_rpc_url, "http://miden-node:57291");
        assert_eq!(config.chain_id, 42);
        assert_eq!(config.bridge_account_id, "0x1234567890abcdef");
    }

    #[test]
    fn test_default_listen_port() {
        let config_str = r#"
            miden_rpc_url = "http://localhost:57291"
            chain_id = 1
            bridge_account_id = "0xabc"
        "#;

        let config = ProxyConfig::from_str(config_str).unwrap();
        assert_eq!(config.listen_port, 8545);
    }

    #[test]
    fn test_default_config() {
        let config = ProxyConfig::default();
        assert_eq!(config.listen_port, 8545);
        assert_eq!(config.miden_rpc_url, "http://localhost:57291");
        assert_eq!(config.chain_id, 1);
    }
}
