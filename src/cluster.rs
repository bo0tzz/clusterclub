use anyhow::{Context, Result};
use memberlist::delegate::CompositeDelegate;
use memberlist::net::NetTransportOptions;
use memberlist::proto::{MaybeResolvedAddress, SecretKey};
use memberlist::tokio::{TokioSocketAddrResolver, TokioTcp, TokioTcpMemberlist};
use memberlist::Options;
use smol_str::SmolStr;
use std::net::SocketAddr;
use std::sync::Arc;

// The memberlist type - explicitly specify delegate with SocketAddr as address type
type Memberlist = TokioTcpMemberlist<SmolStr, TokioSocketAddrResolver, CompositeDelegate<SmolStr, SocketAddr>>;

/// Cluster manager using memberlist for gossip protocol
pub struct ClusterManager {
    memberlist: Arc<Memberlist>,
    backend_count: u32,
    _runtime: tokio::runtime::Runtime, // Keep runtime alive for memberlist background tasks
}

impl ClusterManager {
    /// Create and join a memberlist cluster
    pub fn new(
        listen_port: u16,
        shared_key: String,
        peers: Vec<String>,
        backend_count: u32,
    ) -> Result<Self> {
        // Create a dedicated tokio runtime for memberlist
        let runtime = tokio::runtime::Runtime::new()
            .context("Failed to create tokio runtime for memberlist")?;

        // Initialize memberlist in the runtime
        let (memberlist, member_count) = runtime.block_on(async {
        // Generate a unique node ID
        let node_id = SmolStr::new(format!("node-{}", uuid::Uuid::new_v4()));

        // Parse listen address
        let listen_addr: SocketAddr = format!("0.0.0.0:{}", listen_port)
            .parse()
            .context("Failed to parse listen address")?;

        // Create transport options
        let net_opts =
            NetTransportOptions::<SmolStr, TokioSocketAddrResolver, TokioTcp>::new(node_id.clone())
                .with_bind_addresses([listen_addr].into_iter().collect());

        // Create memberlist options with LAN defaults
        let mut opts = Options::lan();

        // Configure encryption with shared key (AES-256)
        let key_bytes = Self::derive_key(&shared_key);
        opts = opts.with_primary_key(SecretKey::Aes256(key_bytes));

        println!("Encryption enabled with AES-256-GCM");
        println!("Node ID: {}", node_id);
        println!("Listening on: {}", listen_addr);

        // Create delegate (can customize later for metadata)
        let delegate = CompositeDelegate::<SmolStr, SocketAddr>::default();

        // Create memberlist
        let memberlist = TokioTcpMemberlist::with_delegate(delegate, net_opts, opts)
            .await
            .context("Failed to create memberlist")?;

        // Join cluster if peers are provided
        if !peers.is_empty() {
            Self::join_peers(&memberlist, peers).await?;
        } else {
            println!("No peers configured - running as single-node cluster");
        }

            let member_count = memberlist.num_online_members().await;
            Ok::<_, anyhow::Error>((Arc::new(memberlist), member_count))
        })?;

        println!("Cluster initialized with {} members", member_count);

        Ok(ClusterManager {
            memberlist,
            backend_count,
            _runtime: runtime,
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
        memberlist: &TokioTcpMemberlist<SmolStr, TokioSocketAddrResolver, CompositeDelegate<SmolStr, SocketAddr>>,
        peers: Vec<String>,
    ) -> Result<()> {
        use memberlist::net::Node;

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
            let peer_node = Node::new(
                SmolStr::new(format!("peer-{}", addr)),
                MaybeResolvedAddress::Resolved(addr),
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
    pub async fn member_count(&self) -> usize {
        self.memberlist.num_online_members().await
    }

    /// Get local backend count
    pub fn backend_count(&self) -> u32 {
        self.backend_count
    }

    /// Get list of peer node addresses (for detecting if a request is from a peer)
    /// Returns a list of SocketAddr representing cluster member addresses
    pub fn get_peer_addresses(&self) -> Vec<SocketAddr> {
        // Use the memberlist runtime to execute async operations
        self._runtime.block_on(async {
            let members = self.memberlist.members().await;
            members
                .iter()
                .filter_map(|member| {
                    // Get the member's address if it's not the local node
                    if member.id() != self.memberlist.local_id() {
                        Some(*member.address())
                    } else {
                        None
                    }
                })
                .collect()
        })
    }

    // TODO: Implement methods to:
    // - Get list of cluster members with their metadata
    // - Update local metadata when backend count changes (using NodeDelegate)
    // - Derive remote node proxy addresses from memberlist addresses
}
