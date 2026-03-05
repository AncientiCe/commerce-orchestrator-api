# Build stage: compile orchestrator-server (Rust 1.85+ for Cargo.lock v4 and edition2024 deps)
FROM rust:1.85-bookworm AS builder
WORKDIR /app
COPY . .

RUN cargo build -p orchestrator-http --release --bin orchestrator-server

# Runtime stage
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/orchestrator-server /usr/local/bin/orchestrator-server

ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://127.0.0.1:8080/health/live || exit 1

USER nobody
ENTRYPOINT ["/usr/local/bin/orchestrator-server"]
