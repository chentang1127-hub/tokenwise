#!/usr/bin/env bash
# TokenWise Core — Install script for Linux & macOS
# Usage: curl -fsSL https://raw.githubusercontent.com/chentang1127-hub/tokenwise/main/install.sh | bash

set -euo pipefail

BOLD="\033[1m"
GREEN="\033[32m"
RESET="\033[0m"
BLUE="\033[34m"
YELLOW="\033[33m"

REPO="chentang1127-hub/tokenwise"
BIN="tokenwise"
VERSION="${TOKENWISE_VERSION:-latest}"

echo -e "${BOLD}${GREEN}⚡ TokenWise Core${RESET} — Self-hosted LLM execution layer installer"
echo ""

# ── Detect OS / Arch ──────────────────────────────────────
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
    linux)  OS="linux" ;;
    darwin) OS="darwin" ;;
    *)
        echo "Unsupported OS: $OS. TokenWise Core supports Linux and macOS."
        echo "For Windows, use install.ps1 (PowerShell)."
        exit 1
        ;;
esac

case "$ARCH" in
    x86_64|amd64) ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *)
        echo "Unsupported architecture: $ARCH"
        exit 1
        ;;
esac

# ── Download ────────────────────────────────────────────
if [ "$VERSION" = "latest" ]; then
    URL="https://github.com/$REPO/releases/latest/download/tokenwise-${OS}-${ARCH}.tar.gz"
else
    URL="https://github.com/$REPO/releases/download/${VERSION}/tokenwise-${OS}-${ARCH}.tar.gz"
fi

echo "Downloading TokenWise Core ${VERSION} for ${OS}/${ARCH}..."
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

if command -v curl &> /dev/null; then
    curl -fsSL "$URL" -o "$TMPDIR/tokenwise.tar.gz"
elif command -v wget &> /dev/null; then
    wget -q "$URL" -O "$TMPDIR/tokenwise.tar.gz"
else
    echo "Error: need curl or wget to download."
    exit 1
fi

tar -xzf "$TMPDIR/tokenwise.tar.gz" -C "$TMPDIR"

# ── Install ──────────────────────────────────────────────
INSTALL_DIR="${TOKENWISE_INSTALL_DIR:-$HOME/.local/bin}"
mkdir -p "$INSTALL_DIR"
cp "$TMPDIR/tokenwise" "$INSTALL_DIR/tokenwise"
chmod +x "$INSTALL_DIR/tokenwise"
echo -e "  ${GREEN}✓${RESET} Installed to ${BOLD}$INSTALL_DIR/tokenwise${RESET}"

# ── PATH check ──────────────────────────────────────────
if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
    echo ""
    echo -e "  ${YELLOW}⚠${RESET}  ${BOLD}$INSTALL_DIR${RESET} is not in your PATH."
    echo "     Add this to your shell config:"
    echo ""
    echo -e "     ${BLUE}export PATH=\"\$HOME/.local/bin:\$PATH\"${RESET}"
    echo ""
fi

# ── Create default config ────────────────────────────────
CONFIG_DIR="${TOKENWISE_CONFIG_DIR:-$HOME/.config/tokenwise}"
mkdir -p "$CONFIG_DIR"

if [ ! -f "$CONFIG_DIR/config.yaml" ]; then
    cat > "$CONFIG_DIR/config.yaml" << 'EOF'
# TokenWise Core configuration
locale: "en"

proxy:
  listen: "127.0.0.1:9401"
  admin: "127.0.0.1:9400"
  timeout_secs: 120

providers:
  - name: "deepseek"
    base_url: "https://api.deepseek.com/v1"
    api_key_env: "DEEPSEEK_API_KEY"
    models:
      - id: "deepseek-chat"
        tier: "cheap"
        cost_per_1k_prompt: 0.00027
        cost_per_1k_completion: 0.0011
      - id: "deepseek-reasoner"
        tier: "premium"
        cost_per_1k_prompt: 0.00055
        cost_per_1k_completion: 0.00219

  - name: "openai"
    base_url: "https://api.openai.com/v1"
    api_key_env: "OPENAI_API_KEY"
    models:
      - id: "gpt-4.1"
        tier: "premium"
        cost_per_1k_prompt: 0.002
        cost_per_1k_completion: 0.008
      - id: "gpt-4.1-mini"
        tier: "mid"
        cost_per_1k_prompt: 0.0001
        cost_per_1k_completion: 0.0004
      - id: "gpt-4.1-nano"
        tier: "cheap"
        cost_per_1k_prompt: 0.00005
        cost_per_1k_completion: 0.0002

  - name: "openrouter"
    base_url: "https://openrouter.ai/api/v1"
    api_key_env: "OPENROUTER_API_KEY"
    models:
      - id: "openai/gpt-4.1-mini"
        tier: "mid"
        cost_per_1k_prompt: 0.0004
        cost_per_1k_completion: 0.0016
      - id: "google/gemini-2.5-flash"
        tier: "cheap"
        cost_per_1k_prompt: 0.00015
        cost_per_1k_completion: 0.0006
      - id: "anthropic/claude-haiku-4-5"
        tier: "mid"
        cost_per_1k_prompt: 0.0008
        cost_per_1k_completion: 0.004
      - id: "deepseek/deepseek-chat"
        tier: "cheap"
        cost_per_1k_prompt: 0.00027
        cost_per_1k_completion: 0.0011

routing:
  simple_max_tokens: 300
  complex_min_tokens: 1500
  simple_keywords:
    - "summarize"
    - "translate"
    - "extract"
    - "classify"
    - "what is"
    - "define"
    - "list"
    - "convert"
  complex_keywords:
    - "step by step"
    - "think carefully"
    - "reason about"
    - "debug"
    - "implement"
    - "write code"
    - "refactor"
    - "design"
  tier_simple: "cheap"
  tier_complex: "premium"
  tier_default: "mid"

safety_net:
  enabled: true
  max_fallback_retries: 1
  fallback_map:
    cheap: "mid"
    mid: "premium"
  fallback_on_empty_response: true
  fallback_on_truncated: true

license:
  key: "tw_free"

cache:
  ttl_hours: 24
  max_entries: 10000

storage:
  db_path: "./tokenwise.db"
  retention_days: 90

budget:
  daily_limit_usd: 0
  monthly_limit_usd: 0
EOF
    echo -e "  ${GREEN}✓${RESET} Created default config at ${BOLD}$CONFIG_DIR/config.yaml${RESET}"
else
    echo -e "  ${BLUE}ℹ${RESET}  Config already exists at $CONFIG_DIR/config.yaml"
fi

# ── Done ──────────────────────────────────────────────────
echo ""
echo -e "${BOLD}${GREEN}Done!${RESET}"
echo ""
echo -e "  ${BOLD}1.${RESET} Edit config:  ${BLUE}$CONFIG_DIR/config.yaml${RESET}"
echo -e "  ${BOLD}2.${RESET} Start:        ${BLUE}tokenwise start${RESET}"
echo -e "  ${BOLD}3.${RESET} Dashboard:   ${BLUE}http://127.0.0.1:9400${RESET}"
echo ""
echo -e "  Set your app's API base URL to: ${BOLD}http://127.0.0.1:9401/v1${RESET}"
echo ""
