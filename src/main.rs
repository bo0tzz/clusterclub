mod cluster;
mod config;
mod proxy;

use anyhow::Result;
use pingora::server::Server;
use std::env;
use std::sync::Arc;

fn main() -> Result<()> {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} <config-file>", args[0]);
        std::process::exit(1);
    }

    let config_path = &args[1];
    let config = config::Config::from_file(config_path)?;

    println!("ClusterClub starting...");
    println!("  Cluster port: {}", config.cluster.listen_port);
    println!("  Proxy port: {}", config.proxy.listen_port);
    println!("  Local backends: {}", config.backends.len());
    println!("  Peers: {}", config.cluster.peers.len());

    // Initialize cluster (creates its own tokio runtime)
    let backend_count = config.backends.len() as u32;
    let cluster = Arc::new(cluster::ClusterManager::new(
        config.cluster.listen_port,
        config.cluster.shared_key.clone(),
        config.cluster.peers.clone(),
        backend_count,
        config.proxy.listen_port, // Pass proxy port for deriving peer proxy addresses
    )?);

    // Create proxy with local backends
    let backend_addrs: Vec<String> = config
        .backends
        .iter()
        .map(|b| b.address.clone())
        .collect();

    let upstreams = proxy::ClusterProxy::create_load_balancer(backend_addrs)?;

    // Create pingora server
    let mut server = Server::new(None)?;
    server.bootstrap();

    // Add load balancer as background service for health checks
    let lb_service = pingora::services::background::background_service("health_check", upstreams);

    // Get the Arc<LoadBalancer> from the background service
    let lb = lb_service.task();

    server.add_service(lb_service);

    // Create proxy with the shared load balancer and cluster manager
    let proxy = proxy::ClusterProxy::new(lb, cluster);

    // Create HTTP proxy service
    let mut proxy_service = pingora_proxy::http_proxy_service(
        &server.configuration,
        proxy,
    );
    proxy_service.add_tcp(&format!("0.0.0.0:{}", config.proxy.listen_port));

    server.add_service(proxy_service);

    println!("Proxy listening on port {}", config.proxy.listen_port);
    println!("Ready to handle requests!");

    // Run the server
    server.run_forever();
}
