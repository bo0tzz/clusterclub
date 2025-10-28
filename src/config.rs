use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub cluster: ClusterConfig,
    pub proxy: ProxyConfig,
    pub backends: Vec<BackendConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClusterConfig {
    /// Port for memberlist gossip protocol
    pub listen_port: u16,
    /// Shared secret key for cluster authentication/encryption
    pub shared_key: String,
    /// Known peer addresses to join (host:port format)
    #[serde(default)]
    pub peers: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProxyConfig {
    /// Port for client HTTP requests
    pub listen_port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BackendConfig {
    /// Backend address (host:port format)
    pub address: String,
}

impl Config {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read config file: {:?}", path.as_ref()))?;

        let config: Config = toml::from_str(&content)
            .context("Failed to parse TOML configuration")?;

        config.validate()?;
        Ok(config)
    }

    /// Validate configuration values
    fn validate(&self) -> Result<()> {
        // Ensure we have at least one backend
        anyhow::ensure!(!self.backends.is_empty(), "At least one backend must be configured");

        // Validate that all backend addresses can be parsed
        for backend in &self.backends {
            backend.address.parse::<SocketAddr>()
                .with_context(|| format!("Invalid backend address: {}", backend.address))?;
        }

        // Validate peer addresses
        for peer in &self.cluster.peers {
            peer.parse::<SocketAddr>()
                .with_context(|| format!("Invalid peer address: {}", peer))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_validation() {
        let config = Config {
            cluster: ClusterConfig {
                listen_port: 7946,
                shared_key: "test-key".to_string(),
                peers: vec!["192.168.1.10:7946".to_string()],
            },
            proxy: ProxyConfig {
                listen_port: 8080,
            },
            backends: vec![
                BackendConfig {
                    address: "localhost:3003".to_string(),
                },
            ],
        };

        // This should fail because localhost:3003 isn't a valid SocketAddr
        assert!(config.validate().is_err());

        // Fix it to use IP
        let mut config = config;
        config.backends[0].address = "127.0.0.1:3003".to_string();
        assert!(config.validate().is_ok());
    }
}
