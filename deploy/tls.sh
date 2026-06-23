#!/bin/bash
# TokenWise + IPGEO TLS setup — acme.sh + Cloudflare DNS
#
# Prerequisites:
#   1. Cloudflare API token with Zone:DNS:Edit permission
#      Create at: https://dash.cloudflare.com/profile/api-tokens
#   2. Set CF_Token env var before running
#
# Usage: CF_Token=xxx ./deploy/tls.sh
#
# This script:
#   1. Installs acme.sh
#   2. Issues wildcard cert for *.getipgeo.com via DNS challenge
#   3. Installs certs for nginx
#   4. Updates nginx config to serve HTTPS on port 8443
#   5. Sets up cron auto-renewal

set -euo pipefail

DOMAIN="${1:-getipgeo.com}"
CERT_DIR="/etc/ssl/tokenwise"
NGINX_CONF="/opt/ipgeo/nginx.conf"
HTTPS_PORT="${HTTPS_PORT:-8443}"

if [ -z "${CF_Token:-}" ]; then
    echo "❌ CF_Token is required."
    echo "   Create one at https://dash.cloudflare.com/profile/api-tokens"
    echo "   (needs Zone:DNS:Edit permission)"
    echo "   Then run: CF_Token=xxx ./deploy/tls.sh"
    exit 1
fi

echo "🔐 TokenWise TLS Setup — $DOMAIN"
echo ""

# ── 1. Install acme.sh ────────────────────────────────────
if [ ! -f "$HOME/.acme.sh/acme.sh" ]; then
    echo "Installing acme.sh..."
    curl -fsSL https://get.acme.sh | sh -s email="admin@${DOMAIN}"
fi
ACME="$HOME/.acme.sh/acme.sh"

# ── 2. Issue wildcard certificate ─────────────────────────
echo "Issuing wildcard certificate for *.$DOMAIN..."
export CF_Token
$ACME --issue --dns dns_cf -d "$DOMAIN" -d "*.$DOMAIN" --server letsencrypt

# ── 3. Install certificates ──────────────────────────────
echo "Installing certificates to $CERT_DIR..."
mkdir -p "$CERT_DIR"
$ACME --install-cert -d "$DOMAIN" \
    --key-file       "$CERT_DIR/privkey.pem" \
    --fullchain-file "$CERT_DIR/fullchain.pem" \
    --reloadcmd     "docker restart ipgeo-nginx-1"

# ── 4. Update nginx config ────────────────────────────────
if ! grep -q "listen $HTTPS_PORT ssl" "$NGINX_CONF" 2>/dev/null; then
    echo "Adding HTTPS server blocks to nginx config..."
    cp "$NGINX_CONF" "${NGINX_CONF}.bak"

    # Add HTTPS server blocks before the closing }
    # We insert them at the end of the http block
    HTTPS_BLOCK="
    # ══ HTTPS (port $HTTPS_PORT, TLS via acme.sh) ══════════════════

    # IPGEO marketing site
    server {
        listen $HTTPS_PORT ssl;
        server_name getipgeo.com www.getipgeo.com;
        ssl_certificate     $CERT_DIR/fullchain.pem;
        ssl_certificate_key $CERT_DIR/privkey.pem;
        root /app/docs;
        index index.html;
        location / { try_files \$uri \$uri.html \$uri/ =404; }
    }

    # IPGEO API
    server {
        listen $HTTPS_PORT ssl;
        server_name api.getipgeo.com;
        ssl_certificate     $CERT_DIR/fullchain.pem;
        ssl_certificate_key $CERT_DIR/privkey.pem;
        location / {
            proxy_pass http://ipgeo:8000;
            proxy_set_header Host \$host;
            proxy_set_header X-Real-IP \$remote_addr;
            proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
            proxy_set_header X-Forwarded-Proto https;
        }
    }

    # TokenWise Dashboard
    server {
        listen $HTTPS_PORT ssl;
        server_name tw.getipgeo.com;
        ssl_certificate     $CERT_DIR/fullchain.pem;
        ssl_certificate_key $CERT_DIR/privkey.pem;
        location / {
            proxy_pass http://172.17.0.1:9400;
            proxy_set_header Host \$host;
            proxy_set_header X-Real-IP \$remote_addr;
            proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
            proxy_set_header X-Forwarded-Proto https;
            proxy_read_timeout 120s;
        }
    }

    # TokenWise LLM Proxy
    server {
        listen $HTTPS_PORT ssl;
        server_name llm.getipgeo.com;
        ssl_certificate     $CERT_DIR/fullchain.pem;
        ssl_certificate_key $CERT_DIR/privkey.pem;
        location / {
            proxy_pass http://172.17.0.1:9401;
            proxy_set_header Host \$host;
            proxy_set_header X-Real-IP \$remote_addr;
            proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
            proxy_set_header X-Forwarded-Proto https;
            proxy_buffering off;
            proxy_cache off;
            proxy_read_timeout 300s;
            chunked_transfer_encoding on;
        }
    }"

    # Insert before the closing }
    sed -i "/^}/i $HTTPS_BLOCK" "$NGINX_CONF"

    # Add port 8443 mapping to docker-compose
    COMPOSE="/opt/ipgeo/docker-compose.yml"
    if ! grep -q "$HTTPS_PORT:$HTTPS_PORT" "$COMPOSE" 2>/dev/null; then
        cp "$COMPOSE" "${COMPOSE}.bak"
        sed -i "/- \"80:80\"/a\\      - \"$HTTPS_PORT:$HTTPS_PORT\"" "$COMPOSE"
    fi

    echo "Nginx config updated. Restarting..."
    cd /opt/ipgeo && docker compose up -d nginx
fi

# ── 5. Firewall ───────────────────────────────────────────
if command -v ufw &> /dev/null; then
    ufw allow $HTTPS_PORT/tcp 2>/dev/null || true
elif command -v firewall-cmd &> /dev/null; then
    firewall-cmd --add-port=$HTTPS_PORT/tcp --permanent 2>/dev/null || true
    firewall-cmd --reload 2>/dev/null || true
fi

echo ""
echo "✅ TLS Setup Complete!"
echo ""
echo "  Dashboard:  https://tw.getipgeo.com:$HTTPS_PORT"
echo "  Proxy:      https://llm.getipgeo.com:$HTTPS_PORT/v1"
echo ""
echo "  Certificate auto-renews via acme.sh cron job."
echo "  To force renewal: $ACME --renew -d $DOMAIN --force"
