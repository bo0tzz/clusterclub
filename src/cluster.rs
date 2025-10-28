use anyhow::{Context, Result};
use memberlist::agnostic::tokio::TokioRuntime;
use memberlist::quic::{QuicTransport, QuicTransportOptions};
use memberlist::transport::resolver::socket_addr::SocketAddrResolver;
use memberlist::types::SecretKey;
use memberlist::{Memberlist, Options};
use smol_str::SmolStr;
use std::net::SocketAddr;
use std::sync::Arc;

type TokioQuicTransport = QuicTransport<SmolStr, SocketAddrResolver<TokioRuntime>, memberlist::quic::stream_layer::quinn::Quinn<TokioRuntime>, TokioRuntime>;

/// Cluster manager using memberlist for gossip protocol
pub struct ClusterManager {
    memberlist: Arc<Memberlist<TokioQuicTransport>>,
    backend_count: u32,
}

impl ClusterManager {
    /// Create and join a memberlist cluster
    pub async fn new(
        listen_port: u16,
        shared_key: String,
        peers: Vec<String>,
        backend_count: u32,
    ) -> Result<Self> {
        // Generate a unique node ID
        let node_id = SmolStr::new(format!("node-{}", uuid::Uuid::new_v4()));

        // Set listen address
        let listen_addr: SocketAddr = format!("0.0.0.0:{}", listen_port)
            .parse()
            .context("Failed to parse listen address")?;

        // Create QUIC transport options
        let mut transport_opts = QuicTransportOptions::<
            SmolStr,
            SocketAddrResolver<TokioRuntime>,
            memberlist::quic::stream_layer::quinn::Quinn<TokioRuntime>,
        >::with_stream_layer_options(node_id.clone(), Default::default());
        transport_opts.add_bind_address(listen_addr);

        // Create transport
        let transport = QuicTransport::new(transport_opts)
            .await
            .context("Failed to create QUIC transport")?;

        // Create memberlist options with LAN defaults
        let mut opts = Options::lan();

        // Configure encryption with shared key
        // Derive a 32-byte key for AES-256-GCM
        let key_bytes = Self::derive_key(&shared_key);
        opts = opts.with_primary_key(SecretKey::Aes256Gcm(key_bytes));

        println!("Encryption enabled with AES-256-GCM");
        println!("Node ID: {}", node_id);
        println!("Listening on: {}", listen_addr);

        // Create memberlist
        let memberlist = Memberlist::new(transport, opts)
            .await
            .context("Failed to create memberlist")?;

        let memberlist = Arc::new(memberlist);

        // Join cluster if peers are provided
        if !peers.is_empty() {
            Self::join_peers(&memberlist, peers).await?;
        } else {
            println!("No peers configured - running as single-node cluster");
        }

        Ok(ClusterManager {
            memberlist,
            backend_count,
        })
    }

    /// Derive a 32-byte key from the shared secret string
    /// TODO: Use proper KDF like HKDF or PBKDF2 in production
    fn derive_key(shared_key: &str) -> [u8; 32] {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut key = [0u8; 32];

        // Hash the key multiple times to fill 32 bytes
        for i in 0..4 {
            let mut hasher = DefaultHasher::new();
            (shared_key, i).hash(&mut hasher);
            let hash = hasher.finish().to_le_bytes();
            key[i * 8..(i + 1) * 8].copy_from_slice(&hash);
        }

        key
    }

    /// Attempt to join cluster by contacting peers
    async fn join_peers(
        memberlist: &Memberlist<TokioQuicTransport>,
        peers: Vec<String>,
    ) -> Result<()> {
        let peer_addrs: Vec<SocketAddr> = peers
            .iter()
            .map(|p| {
                p.parse()
                    .with_context(|| format!("Failed to parse peer address: {}", p))
            })
            .collect::<Result<Vec<_>>>()?;

        println!("Attempting to join cluster via {} peers...", peer_addrs.len());

        // Try to join via each peer
        for addr in peer_addrs {
            // Create a node reference for the peer
            let peer_node = memberlist::types::Node::new(
                SmolStr::new(format!("peer-{}", addr)),
                memberlist::types::MaybeResolvedAddress::Resolved(addr),
            );

            match memberlist.join(peer_node).await {
                Ok(_) => {
                    println!("✓ Successfully joined cluster via {}", addr);
                    return Ok(());
                }
                Err(e) => {
                    println!("✗ Failed to join via {}: {}", addr, e);
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
