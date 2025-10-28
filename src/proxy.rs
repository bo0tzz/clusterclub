use anyhow::Result;
use async_trait::async_trait;
use pingora::prelude::*;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_load_balancing::{
    health_check::TcpHealthCheck,
    selection::RoundRobin,
    LoadBalancer,
};
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
        // Weighted round-robin selection across all backends (local + remote)
        // Weight: local backends count individually, each remote peer represents their backends

        let peer_proxies = self.cluster.get_peer_proxy_addresses().await;
        let local_backend_count = self.cluster.backend_count() as usize;

        // Total weight = local backend count + number of remote peers
        // (each remote peer is weighted as 1 for now, will use dynamic metadata later)
        let total_weight = local_backend_count + peer_proxies.len();

        if total_weight == 0 {
            return Err(Error::new(ErrorType::ConnectError));
        }

        // Round-robin across total weight
        let request_num = self.request_counter.fetch_add(1, Ordering::Relaxed);
        let target_idx = request_num % total_weight;

        if target_idx < local_backend_count {
            // Route to local backend
            let upstream = self
                .local_lb
                .select(b"", 256)
                .ok_or_else(|| Error::new(ErrorType::ConnectError))?;

            println!("→ Routing to local backend");
            Ok(Box::new(HttpPeer::new(upstream, false, String::new())))
        } else {
            // Route to remote peer
            let peer_idx = target_idx - local_backend_count;
            let peer_addr = &peer_proxies[peer_idx];

            println!("→ Routing to remote peer: {}", peer_addr);
            Ok(Box::new(HttpPeer::new(
                peer_addr.as_str(),
                false,
                String::new(),
            )))
        }
    }

    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        _upstream_request: &mut RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<(), Box<Error>> {
        // Preserve original Host header
        // The backend should see the original host, not our internal address
        Ok(())
    }
}
