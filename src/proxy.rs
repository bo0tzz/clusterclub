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
use std::sync::Arc;

use crate::cluster::ClusterManager;

pub struct ClusterProxy {
    local_lb: Arc<LoadBalancer<RoundRobin>>,
    cluster: Arc<ClusterManager>,
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
        ClusterProxy { local_lb, cluster }
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
        // Check if the request is from a peer node
        let _is_peer_request = if let Some(client_addr) = session.client_addr() {
            let peer_ips = self.cluster.get_peer_addresses();
            let client_ip = client_addr.as_inet().map(|addr| addr.ip());

            let is_peer = client_ip
                .map(|ip| peer_ips.iter().any(|peer| peer.ip() == ip))
                .unwrap_or(false);

            if is_peer {
                println!("🔗 Peer request detected from: {}", client_addr);
            } else {
                println!("👤 Client request from: {}", client_addr);
            }

            is_peer
        } else {
            false
        };

        // For now, always route to local backends
        // TODO: If not a peer request, implement two-tier selection (local + remote peers)
        let upstream = self
            .local_lb
            .select(b"", 256)
            .ok_or_else(|| Error::new(ErrorType::ConnectError))?;

        let peer = Box::new(HttpPeer::new(upstream, false, String::new()));
        Ok(peer)
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
