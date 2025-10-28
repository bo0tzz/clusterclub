mod config;

use anyhow::Result;
use std::env;

fn main() -> Result<()> {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} <config-file>", args[0]);
        std::process::exit(1);
    }

    let config_path = &args[1];
    let config = config::Config::from_file(config_path)?;

    println!("Loaded configuration:");
    println!("  Cluster port: {}", config.cluster.listen_port);
    println!("  Proxy port: {}", config.proxy.listen_port);
    println!("  Backends: {}", config.backends.len());
    println!("  Peers: {}", config.cluster.peers.len());

    Ok(())
}
