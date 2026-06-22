# ── TokenWise Core — Production Dockerfile ─────────────────
# Multi-stage: Rust → Alpine. Final image ~15 MB, fully static.
#
# Build:   docker build -t tokenwise .
# Run:     docker run -p 9400-9401:9400-9401 -v ./data:/app tokenwise
# Compose: docker compose up -d

FROM rust:1.96-alpine AS builder

RUN apk add --no-cache musl-dev pkgconfig

WORKDIR /app

# Layer 1: fetch dependencies (cache-friendly — only re-runs if Cargo.toml changes)
COPY Cargo.toml Cargo.lock* ./
RUN mkdir -p src templates \
    && echo 'fn main() {}' > src/main.rs \
    && echo '' > src/lib.rs
RUN cargo build --release 2>/dev/null || true
RUN rm -rf src

# Layer 2: compile release binary
COPY src/ src/
COPY templates/ templates/
RUN cargo build --release \
    && strip target/release/tokenwise

# ── Runtime stage ──────────────────────────────────────────
FROM alpine:3.21

RUN apk add --no-cache ca-certificates tzdata curl

# Non-root user
RUN adduser -D -h /app tokenwise

COPY --from=builder /app/target/release/tokenwise /usr/local/bin/tokenwise

USER tokenwise
WORKDIR /app

EXPOSE 9400 9401

# Health check via dashboard endpoint
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD curl -fs http://127.0.0.1:9400/health || exit 1

ENTRYPOINT ["tokenwise"]
CMD ["start", "--config", "/app/config.yaml"]
