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

pub struct ClusterProxy {
    local_lb: Arc<LoadBalancer<RoundRobin>>,
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

    pub fn new(local_lb: Arc<LoadBalancer<RoundRobin>>) -> Self {
        ClusterProxy { local_lb }
    }
}

#[async_trait]
impl ProxyHttp for ClusterProxy {
    type CTX = ();

    fn new_ctx(&self) -> Self::CTX {}

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>, Box<Error>> {
        // For now, just select from local backends
        // TODO: Implement cluster-wide load balancing
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
