# TokenWise Core — Deployment Guide

This guide covers deploying TokenWise Core to a VPS or dedicated server.
Pick the option that fits your setup.

---

## Option A: Docker Compose (recommended)

Zero-dependency deployment. One `docker compose up -d` and you're live.

### Prerequisites

- Docker Engine 24+ and Docker Compose v2
- A domain name pointing to your server (for TLS)

### Steps

```bash
# 1. Clone or copy the deployment files
git clone https://github.com/chentang1127-hub/tokenwise
cd tokenwise

# 2. Edit config.yaml — add API keys for your providers
#    Or leave keys blank: TokenWise will forward your client's Authorization header.
vim config.yaml

# 3. Set your domain names (optional, for auto-TLS)
#    Without this, Caddy serves on :80 with no TLS.
export CADDY_DASHBOARD_DOMAIN=dashboard.example.com
export CADDY_PROXY_DOMAIN=proxy.example.com

# 4. Start
docker compose up -d

# 5. Verify
curl http://localhost:9400/health
# → {"status":"ok","version":"0.5.0","db":"connected","uptime_seconds":0}
```

### File Layout

```
.
├── config.yaml          # Your provider config (mount as read-only)
├── Caddyfile            # Caddy reverse proxy config
├── docker-compose.yml   # tokenwise + caddy services
└── tokenwise.db         # SQLite database (auto-created, persisted via volume)
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `TW_HEADLESS` | `true` | Set true for Docker — skips browser auto-open |
| `TW_PROXY_LISTEN` | `0.0.0.0:9401` | Proxy bind address |
| `TW_PROXY_ADMIN` | `0.0.0.0:9400` | Dashboard bind address |
| `TW_DB_PATH` | `/app/tokenwise.db` | SQLite path |
| `TW_LOCALE` | `en` | `en` or `zh` |
| `TW_BUDGET_DAILY` | — | Daily spending cap in USD |
| `TW_BUDGET_MONTHLY` | — | Monthly spending cap in USD |
| `TW_CACHE_TTL` | — | Cache TTL in hours |
| `RUST_LOG` | `info,tokenwise=debug` | Log level |
| `CADDY_DASHBOARD_DOMAIN` | `:80` | Dashboard domain |
| `CADDY_PROXY_DOMAIN` | `:80` | Proxy domain |

### Upgrading

```bash
docker compose pull       # Pull latest image (if using registry)
docker compose up -d      # Recreate with new image
# Data persists in the tokenwise_data volume
```

### Backup

```bash
# Copy the database out of the volume
docker compose exec tokenwise tokenwise backup --output /app/backups
docker compose cp tokenwise:/app/backups ./backups
```

---

## Option B: systemd (bare-metal / VPS without Docker)

Direct binary deployment with systemd supervision.

### Prerequisites

- Linux server (Ubuntu 22.04/24.04, Debian 12, etc.)
- Rust toolchain (if building from source) or download prebuilt binary

### Steps

```bash
# 1. Create the tokenwise user
sudo useradd -r -s /bin/false -d /var/lib/tokenwise -m tokenwise

# 2. Install the binary
#    From source:
git clone https://github.com/chentang1127-hub/tokenwise
cd tokenwise
cargo build --release
sudo cp target/release/tokenwise /usr/local/bin/

#    Or download prebuilt (from GitHub Releases):
# curl -fsSL https://github.com/chentang1127-hub/tokenwise/releases/latest/download/tokenwise-linux-amd64.tar.gz | sudo tar xz -C /usr/local/bin/

# 3. Create config
sudo mkdir -p /var/lib/tokenwise
sudo cp config.yaml /var/lib/tokenwise/
sudo chown -R tokenwise:tokenwise /var/lib/tokenwise

# 4. Install systemd unit
sudo cp deploy/tokenwise.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now tokenwise

# 5. Verify
sudo systemctl status tokenwise
curl http://localhost:9400/health
```

### systemd Unit Features

- `User=tokenwise` — runs as non-root
- `ProtectSystem=strict` — read-only access to system directories
- `ProtectHome=true` — no access to /home
- `NoNewPrivileges=true` — privilege escalation blocked
- `Restart=on-failure` — auto-restart on crash
- `WorkingDirectory=/var/lib/tokenwise` — database and config live here

### Logs

```bash
sudo journalctl -u tokenwise -f     # Follow logs
sudo journalctl -u tokenwise -n 100 # Last 100 lines
```

---

## Option C: Caddy + systemd (bare-metal with TLS)

For production with HTTPS without Docker.

### Prerequisites

- Caddy installed: `sudo apt install caddy` or https://caddyserver.com/docs/install

### Steps

```bash
# 1. Follow Option B to install TokenWise as a systemd service

# 2. Copy Caddyfile to Caddy config directory
sudo cp Caddyfile /etc/caddy/Caddyfile.tokenwise

# 3. Edit /etc/caddy/Caddyfile — set your domains:
#    Replace the :80 placeholders with your actual domains.
#    Caddy auto-obtains Let's Encrypt certificates.

# 4. Import into Caddy's main config or replace default
sudo cp /etc/caddy/Caddyfile.tokenwise /etc/caddy/Caddyfile
sudo systemctl reload caddy

# 5. Verify TLS
curl https://dashboard.example.com/health
```

### Caddyfile Reference

```
# Dashboard virtual host
{$CADDY_DASHBOARD_DOMAIN: :80} {
    reverse_proxy 127.0.0.1:9400
    encode gzip
}

# Proxy virtual host
{$CADDY_PROXY_DOMAIN: :80} {
    reverse_proxy 127.0.0.1:9401
    encode gzip
}
```

---

## Firewall

TokenWise only needs ports 80/443 open to the internet (via Caddy or nginx).
Internal ports 9400/9401 should be firewall-blocked from the outside.

```bash
# ufw example
sudo ufw allow 80/tcp
sudo ufw allow 443/tcp
sudo ufw deny 9400/tcp   # Only localhost needs this
sudo ufw deny 9401/tcp   # Only localhost needs this
sudo ufw enable
```

---

## Monitoring

### Health Check

```bash
# JSON response with status
curl http://localhost:9400/health
# {"status":"ok","version":"0.5.0","db":"connected","uptime_seconds":86400,"routing_enabled":true}
```

### Status CLI

```bash
tokenwise status
# TokenWise Core v0.5.0
#   Admin Dashboard (0.0.0.0:9400):  ✅ running
#   Proxy           (0.0.0.0:9401):  ✅ running
#   Database:               /var/lib/tokenwise/tokenwise.db
#     Size: 245.3 KB
#   License tier:           Pro
```

### Prometheus

Point your Prometheus scraper at `http://localhost:9400/metrics`:

```yaml
# prometheus.yml
scrape_configs:
  - job_name: tokenwise
    static_configs:
      - targets: ['localhost:9400']
```

Available metrics:
- `tokenwise_requests_total` — total API calls
- `tokenwise_cache_hits_total` — cache-served responses
- `tokenwise_routing_decisions_total` — smart routing decisions
- `tokenwise_empty_streams_total` — empty stream detections
- `tokenwise_truncated_streams_total` — truncated stream detections
- `tokenwise_cost_usd_total` — total spend
- `tokenwise_cache_hit_ratio` — cache effectiveness

---

## Backup

```bash
# WAL checkpoint + timestamped copy
tokenwise backup --output /var/backups/tokenwise

# Cron: daily backup at 3am
# 0 3 * * * /usr/local/bin/tokenwise backup --output /var/backups/tokenwise

# Restore: just copy the .db file back
cp /var/backups/tokenwise/tokenwise.db.20260115_030000.bak /var/lib/tokenwise/tokenwise.db
```

---

## Troubleshooting

| Symptom | Check |
|---------|-------|
| 502 Bad Gateway | TokenWise not running — `systemctl status tokenwise` |
| SSL certificate error | DNS not pointing to server — `dig dashboard.example.com` |
| DB locked | Restart to force WAL checkpoint: `systemctl restart tokenwise` |
| High memory | Check cache size in config: `max_entries: 1000` |
| Budget exceeded | Increase limits in `config.yaml` or set to `0` (unlimited) |
