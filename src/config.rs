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
    /// Address to advertise to other cluster members
    pub advertise_address: Option<String>,
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

        // Validate that all backend addresses have host:port format
        for backend in &self.backends {
            Self::validate_address(&backend.address)
                .with_context(|| format!("Invalid backend address: {}", backend.address))?;
        }

        // Validate peer addresses
        for peer in &self.cluster.peers {
            Self::validate_address(peer)
                .with_context(|| format!("Invalid peer address: {}", peer))?;
        }

        Ok(())
    }

    /// Validate that an address is in host:port format
    fn validate_address(addr: &str) -> Result<()> {
        // Try parsing as SocketAddr first (for IP:port)
        if addr.parse::<SocketAddr>().is_ok() {
            return Ok(());
        }

        // Otherwise validate hostname:port format
        let parts: Vec<&str> = addr.rsplitn(2, ':').collect();
        anyhow::ensure!(parts.len() == 2, "Address must be in host:port format");

        let port = parts[0];
        let host = parts[1];

        anyhow::ensure!(!host.is_empty(), "Host cannot be empty");
        port.parse::<u16>()
            .with_context(|| format!("Invalid port number: {}", port))?;

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
                advertise_address: None,
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

        // Both hostnames and IPs should be valid
        assert!(config.validate().is_ok());

        let mut config = config;
        config.backends[0].address = "127.0.0.1:3003".to_string();
        assert!(config.validate().is_ok());

        // Test invalid formats
        config.backends[0].address = "invalid".to_string();
        assert!(config.validate().is_err());

        config.backends[0].address = "host:notaport".to_string();
        assert!(config.validate().is_err());
    }
}
