use anyhow::{Context, Result};
use memberlist::net::Transport;
use memberlist::quic::QuicTransport;
use memberlist::{Memberlist, Options};
use std::net::SocketAddr;
use std::sync::Arc;

/// Cluster manager using memberlist for gossip protocol
pub struct ClusterManager {
    memberlist: Arc<Memberlist<QuicTransport>>,
    backend_count: u32,
}

impl ClusterManager {
    /// Create and join a memberlist cluster
    pub async fn new(
        listen_port: u16,
        _shared_key: String,
        peers: Vec<String>,
        backend_count: u32,
    ) -> Result<Self> {
        // Create memberlist options with LAN defaults
        let opts = Options::lan();

        // Set listen address
        let listen_addr: SocketAddr = format!("0.0.0.0:{}", listen_port)
            .parse()
            .context("Failed to parse listen address")?;

        // TODO: Configure encryption with shared_key
        // Need to determine correct API for SecretKey and EncryptionAlgo
        // For now, QUIC transport provides encryption at transport layer

        println!("Creating QUIC transport on {}", listen_addr);

        // Create QUIC transport
        let transport = QuicTransport::new(listen_addr, Default::default())
            .await
            .context("Failed to create QUIC transport")?;

        // Create memberlist
        let memberlist = Memberlist::new(transport, opts)
            .await
            .context("Failed to create memberlist")?;

        let memberlist = Arc::new(memberlist);

        // Join cluster if peers are provided
        if !peers.is_empty() {
            Self::join_peers(&memberlist, peers).await?;
        }

        Ok(ClusterManager {
            memberlist,
            backend_count,
        })
    }

    /// Attempt to join cluster by contacting peers
    async fn join_peers(
        memberlist: &Memberlist<QuicTransport>,
        peers: Vec<String>,
    ) -> Result<()> {
        let peer_addrs: Vec<SocketAddr> = peers
            .iter()
            .map(|p| {
                p.parse()
                    .with_context(|| format!("Failed to parse peer address: {}", p))
            })
            .collect::<Result<Vec<_>>>()?;

        // Try to join via each peer
        for addr in peer_addrs {
            match memberlist.join(addr).await {
                Ok(_) => {
                    println!("Successfully joined cluster via {}", addr);
                    return Ok(());
                }
                Err(e) => {
                    println!("Failed to join via {}: {}", addr, e);
                    // Continue trying other peers
                }
            }
        }

        // If we get here, all join attempts failed
        // This is not fatal - we'll run as a single-node cluster
        println!("Could not join any peers, running as single-node cluster");
        Ok(())
    }

    /// Get the number of online cluster members
    pub fn member_count(&self) -> usize {
        self.memberlist.num_online_members()
    }

    /// Get local backend count
    pub fn backend_count(&self) -> u32 {
        self.backend_count
    }

    // TODO: Implement methods to:
    // - Get list of cluster members with their metadata
    // - Update local metadata when backend count changes
    // - Derive remote node proxy addresses from memberlist addresses
}
