# Build stage
FROM rust:1.90-slim AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    cmake \
    g++ \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src

# Build the application in release mode
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy the binary from builder
COPY --from=builder /build/target/release/clusterclub /usr/local/bin/clusterclub

# Create directory for config files
RUN mkdir -p /etc/clusterclub

WORKDIR /etc/clusterclub

# Run as non-root user
RUN useradd -m -u 1000 clusterclub && \
    chown -R clusterclub:clusterclub /etc/clusterclub
USER clusterclub

ENTRYPOINT ["/usr/local/bin/clusterclub"]
CMD ["/etc/clusterclub/config.toml"]
