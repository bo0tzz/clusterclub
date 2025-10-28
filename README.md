# ClusterClub

A distributed load balancer built with Rust, using Pingora for proxying and Memberlist for cluster membership.

## Features

- **Local Load Balancing**: Round-robin load balancing across local backends
- **Health Checks**: Automatic TCP health checking (10-second intervals)
- **Distributed Clustering**: (Coming soon) Use memberlist to form clusters and share backends across nodes

## Current Status

✅ Basic pingora proxy with local backends
✅ Health checks with automatic failover
🚧 Memberlist cluster integration
🚧 Source IP-based routing detection
🚧 Cluster-wide load balancing

## Building

```bash
cargo build --release
```

## Configuration

Create a `config.toml` file (see `config.example.toml`):

```toml
[cluster]
listen_port = 7946
shared_key = "your-secret-key-here"
peers = []  # Add peer addresses like "192.168.1.10:7946"

[proxy]
listen_port = 8080

[[backends]]
address = "127.0.0.1:3003"

[[backends]]
address = "127.0.0.1:3004"
```

## Running

```bash
./target/release/clusterclub config.toml
```

## Testing Locally

1. Start some test backend servers:
```bash
python3 test_backend.py 3003 &
python3 test_backend.py 3004 &
```

2. Start the proxy:
```bash
./target/debug/clusterclub config.example.toml
```

3. Test it:
```bash
curl http://localhost:8080/
```

You should see round-robin responses from different backend ports.

## Architecture

### Phase 1: Local Proxy (Current)
- Pingora HTTP proxy
- Round-robin load balancing to local backends
- TCP health checks

### Phase 2: Distributed Cluster (In Progress)
- Memberlist gossip protocol for cluster membership
- Nodes share backend counts via metadata
- Two-tier load balancing:
  1. Select node (weighted by backend count)
  2. Select backend on that node
- Loop prevention via source IP detection

## TODO

- [ ] Implement memberlist cluster integration
- [ ] Add source IP-based routing detection
- [ ] Create custom ServiceDiscovery for memberlist
- [ ] Integrate two-tier load balancing (local vs cluster)
- [ ] TODO: Dynamically update metadata to reflect healthy backend count (currently static)
