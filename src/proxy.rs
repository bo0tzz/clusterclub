use anyhow::Result;
use async_trait::async_trait;
use pingora::prelude::*;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_load_balancing::{health_check::TcpHealthCheck, selection::RoundRobin, LoadBalancer};
use pingora_proxy::{ProxyHttp, Session};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::cluster::ClusterManager;

pub struct ClusterProxy {
    local_lb: Arc<LoadBalancer<RoundRobin>>,
    cluster: Arc<ClusterManager>,
    request_counter: AtomicUsize, // For round-robin across local/remote
}

impl ClusterProxy {
    pub fn create_load_balancer(local_backends: Vec<String>) -> Result<LoadBalancer<RoundRobin>> {
        // Create load balancer with local backends
        let mut upstreams = LoadBalancer::try_from_iter(local_backends)?;

        // Configure health checks
        let hc = TcpHealthCheck::new();
        upstreams.set_health_check(hc);
        upstreams.health_check_frequency = Some(std::time::Duration::from_secs(10));

        // Disable service discovery updates (we manage backends statically for now)
        upstreams.update_frequency = None;

        Ok(upstreams)
    }

    pub fn new(local_lb: Arc<LoadBalancer<RoundRobin>>, cluster: Arc<ClusterManager>) -> Self {
        ClusterProxy {
            local_lb,
            cluster,
            request_counter: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl ProxyHttp for ClusterProxy {
    type CTX = ();

    fn new_ctx(&self) -> Self::CTX {}

    async fn upstream_peer(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>, Box<Error>> {
        // Check if request is from a peer node by comparing source IP (not port)
        // If so, only route to local backends to prevent infinite loops
        let source_ip = session.client_addr();
        let from_peer = if let Some(addr) = source_ip {
            let peer_addrs = self.cluster.get_peer_addresses().await;
            // Compare only IP addresses, not ports (source port is ephemeral)
            // Parse both to std::net and compare IPs
            if let Ok(std_addr) = addr.to_string().parse::<std::net::SocketAddr>() {
                peer_addrs
                    .iter()
                    .any(|peer_addr| peer_addr.ip() == std_addr.ip())
            } else {
                false
            }
        } else {
            false
        };

        if from_peer {
            // Request came from another cluster node - only route to local backends
            tracing::debug!(
                source_ip = ?source_ip,
                "Request from peer detected, routing to local backend only"
            );

            let upstream = self.local_lb.select(b"", 256).ok_or_else(|| {
                tracing::error!("No healthy local backends available");
                Error::new(ErrorType::ConnectError)
            })?;

            tracing::info!(backend = ?upstream, "→ Routing to local backend (loop prevention)");
            return Ok(Box::new(HttpPeer::new(upstream, false, String::new())));
        }

        // Weighted round-robin selection across all backends (local + remote)
        // Weight: local backends count individually, peers weighted by their backend counts
        let peer_info = self.cluster.get_peer_proxy_info().await;
        let local_backend_count = self.cluster.backend_count() as usize;

        // Calculate total weight: local backends + sum of peer backend counts
        let peer_total_weight: usize = peer_info.iter().map(|(_, count)| *count as usize).sum();
        let total_weight = local_backend_count + peer_total_weight;

        tracing::debug!(
            local_backend_count = local_backend_count,
            peer_total_weight = peer_total_weight,
            total_weight = total_weight,
            peer_count = peer_info.len(),
            "Calculated routing weights"
        );

        if total_weight == 0 {
            tracing::error!("No backends available (local or remote)");
            return Err(Error::new(ErrorType::ConnectError));
        }

        // Round-robin across total weight
        let request_num = self.request_counter.fetch_add(1, Ordering::Relaxed);
        let target_idx = request_num % total_weight;

        tracing::debug!(
            request_num = request_num,
            target_idx = target_idx,
            "Selected routing target"
        );

        if target_idx < local_backend_count {
            // Route to local backend
            let upstream = self.local_lb.select(b"", 256).ok_or_else(|| {
                tracing::error!("No healthy local backends available");
                Error::new(ErrorType::ConnectError)
            })?;

            tracing::info!(backend = ?upstream, "→ Routing to local backend");
            Ok(Box::new(HttpPeer::new(upstream, false, String::new())))
        } else {
            // Route to remote peer - find which peer based on weight
            let mut weight_offset = local_backend_count;
            for (peer_addr, backend_count) in peer_info {
                let peer_weight = backend_count as usize;
                if target_idx < weight_offset + peer_weight {
                    tracing::info!(
                        peer = %peer_addr,
                        backend_count = backend_count,
                        "→ Routing to remote peer"
                    );

                    return Ok(Box::new(HttpPeer::new(
                        peer_addr.as_str(),
                        false,
                        String::new(),
                    )));
                }
                weight_offset += peer_weight;
            }

            // This should never happen, but handle it gracefully
            tracing::error!("Weight calculation error in peer selection");
            Err(Error::new(ErrorType::ConnectError))
        }
    }
}
