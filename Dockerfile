# ── Build stage ──────────────────────────────────────────
FROM rust:1.96-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/
COPY templates/ templates/

RUN cargo build --release && \
    strip target/release/tokenwise

# ── Runtime stage ────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/tokenwise /usr/local/bin/tokenwise
COPY config.yaml /etc/tokenwise/config.yaml
COPY config.cn.yaml /etc/tokenwise/config.cn.yaml

EXPOSE 9400 9401

VOLUME ["/data"]

ENV TOKENWISE_DB_PATH=/data/tokenwise.db

ENTRYPOINT ["tokenwise"]
CMD ["--config", "/etc/tokenwise/config.yaml", "start"]
