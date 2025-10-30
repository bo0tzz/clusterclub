use anyhow::{Context, Result};
use memberlist::delegate::CompositeDelegate;
use memberlist::net::NetTransportOptions;
use memberlist::proto::{EncryptionAlgorithm, MaybeResolvedAddress, SecretKey, SecretKeys};
use memberlist::tokio::{TokioSocketAddrResolver, TokioTcp, TokioTcpMemberlist};
use memberlist::Options;
use smol_str::SmolStr;
use std::net::SocketAddr;
use std::sync::Arc;

// The memberlist type - explicitly specify delegate with SocketAddr as address type
type Memberlist =
    TokioTcpMemberlist<SmolStr, TokioSocketAddrResolver, CompositeDelegate<SmolStr, SocketAddr>>;

/// Cluster manager using memberlist for gossip protocol
pub struct ClusterManager {
    memberlist: Arc<Memberlist>,
    backend_count: u32,
    proxy_port: u16, // Port where peer proxies listen for HTTP requests
}

impl ClusterManager {
    /// Create and join a memberlist cluster
    pub fn new(
        advertise_address: Option<String>,
        listen_port: u16,
        shared_key: String,
        peers: Vec<String>,
        backend_count: u32,
        proxy_port: u16,
    ) -> Result<Self> {
        // Create a dedicated tokio runtime for memberlist and leak it
        // This prevents the runtime from being dropped in an async context
        let runtime = Box::leak(Box::new(
            tokio::runtime::Runtime::new()
                .context("Failed to create tokio runtime for memberlist")?,
        ));

        let (memberlist, member_count) = runtime.block_on(async {
            let node_id = SmolStr::new(format!("node-{}", uuid::Uuid::new_v4()));

            let listen_addr: SocketAddr = format!("0.0.0.0:{}", listen_port)
                .parse()
                .context("Failed to parse listen address")?;

            tracing::info!(node_id = %node_id, "Node ID generated");
            tracing::info!(listen_addr = %listen_addr, "Listening on address");

            let mut net_opts =
                NetTransportOptions::<SmolStr, TokioSocketAddrResolver, TokioTcp>::new(
                    node_id.clone(),
                );

            //if advertise_address, parse it to socketaddr and add_bind_address
            if let Some(addr_str) = advertise_address {
                let advertise_addr: SocketAddr = format!("{}:{}", addr_str, listen_port)
                    .parse()
                    .context("Failed to parse advertise address")?;
                net_opts.add_bind_address(advertise_addr);
            }

            net_opts.add_bind_address(listen_addr);

            let mut opts = Options::lan();

            let key_bytes = Self::derive_key(&shared_key);
            let secret_key = SecretKey::Aes256(key_bytes);

            let mut secret_keys = SecretKeys::new();
            secret_keys.push(secret_key);

            opts = opts
                .with_primary_key(secret_key)
                .with_secret_keys(secret_keys)
                .with_encryption_algo(EncryptionAlgorithm::NoPadding)
                .with_gossip_verify_incoming(true)
                .with_gossip_verify_outgoing(true);

            let delegate = CompositeDelegate::<SmolStr, SocketAddr>::default();

            let memberlist = TokioTcpMemberlist::with_delegate(delegate, net_opts, opts)
                .await
                .context("Failed to create memberlist")?;

            let advertise_address = memberlist.advertise_address();
            tracing::info!(advertise_address = %advertise_address, "Memberlist running with");

            // Join cluster if peers are provided
            if !peers.is_empty() {
                Self::join_peers(&memberlist, peers).await?;
            } else {
                tracing::info!("No peers configured - running as single-node cluster");
            }

            let member_count = memberlist.num_online_members().await;
            Ok::<_, anyhow::Error>((Arc::new(memberlist), member_count))
        })?;

        tracing::info!(member_count = member_count, "Cluster initialized");

        Ok(ClusterManager {
            memberlist,
            backend_count,
            proxy_port,
        })
    }

    /// Derive a 32-byte key from the shared secret string using HKDF-SHA256
    fn derive_key(shared_key: &str) -> [u8; 32] {
        use hkdf::Hkdf;
        use sha2::Sha256;

        let salt = b"clusterclub-memberlist-encryption";
        let info = b"AES-256-GCM-key";

        let hkdf = Hkdf::<Sha256>::new(Some(salt), shared_key.as_bytes());
        let mut key = [0u8; 32];
        hkdf.expand(info, &mut key)
            .expect("HKDF expansion failed - this should never happen");

        key
    }

    //TODO: Refactor to resolve_peers and use memberlist.join_many
    async fn join_peers(
        memberlist: &TokioTcpMemberlist<
            SmolStr,
            TokioSocketAddrResolver,
            CompositeDelegate<SmolStr, SocketAddr>,
        >,
        peers: Vec<String>,
    ) -> Result<()> {
        use memberlist::net::Node;
        use tokio::net::lookup_host;

        let mut peer_addrs = Vec::new();
        for peer in peers {
            if let Ok(addr) = peer.parse::<SocketAddr>() {
                peer_addrs.push(addr);
            } else {
                let resolved = lookup_host(&peer)
                    .await
                    .with_context(|| format!("Failed to resolve peer address: {}", peer))?
                    .next()
                    .with_context(|| format!("No addresses found for peer: {}", peer))?;
                peer_addrs.push(resolved);
            }
        }

        tracing::info!(peer_count = peer_addrs.len(), "Attempting to join cluster");

        for addr in peer_addrs {
            let peer_node = Node::new(
                SmolStr::new(format!("peer-{}", addr)),
                MaybeResolvedAddress::Resolved(addr),
            );

            match memberlist.join(peer_node).await {
                Ok(_) => {
                    tracing::info!(peer_addr = %addr, "Successfully joined cluster");
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!(peer_addr = %addr, error = %e, "Failed to join via peer");
                }
            }
        }

        tracing::info!("Could not join any peers, running as single-node cluster");
        Ok(())
    }

    pub fn backend_count(&self) -> u32 {
        self.backend_count
    }

    pub async fn get_peer_addresses(&self) -> Vec<SocketAddr> {
        let members = self.memberlist.members().await;
        members
            .iter()
            .filter_map(|member| {
                if member.id() != self.memberlist.local_id() {
                    Some(*member.address())
                } else {
                    None
                }
            })
            .collect()
    }

    pub async fn get_peer_proxy_info(&self) -> Vec<(String, u32)> {
        let members = self.memberlist.members().await;
        let local_id = self.memberlist.local_id();

        tracing::debug!(
            total_members = members.len(),
            local_id = %local_id,
            "Retrieving peer proxy info from memberlist"
        );

        for member in &members {
            tracing::debug!(
                member_id = %member.id(),
                member_addr = %member.address(),
                is_local = (member.id() == local_id),
                "Inspecting cluster member"
            );
        }

        let peers: Vec<_> = members
            .iter()
            .filter_map(|member| {
                let is_local = member.id() == local_id;

                tracing::debug!(
                    member_id = %member.id(),
                    local_id = %local_id,
                    is_local = is_local,
                    "Comparing member ID with local ID"
                );

                if !is_local {
                    let cluster_addr = member.address();
                    let proxy_addr = format!("{}:{}", cluster_addr.ip(), self.proxy_port);

                    // For now, assume peers have same backend count as us
                    // TODO: Implement NodeDelegate to share actual backend counts
                    let backend_count = self.backend_count;

                    tracing::debug!(
                        peer_id = %member.id(),
                        peer_addr = %proxy_addr,
                        backend_count = backend_count,
                        "Found peer in cluster"
                    );

                    Some((proxy_addr, backend_count))
                } else {
                    tracing::debug!(member_id = %member.id(), "Skipping local node from peer list");
                    None
                }
            })
            .collect();

        tracing::info!(peer_count = peers.len(), "Peer proxy addresses retrieved");
        peers
    }
}
