# ⚡ TokenWise Core

**Self-hosted LLM execution layer. One binary, zero code changes.**

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/chentang1127-hub/tokenwise/actions/workflows/ci.yml/badge.svg)](https://github.com/chentang1127-hub/tokenwise/actions/workflows/ci.yml)

---

TokenWise Core is a **transparent proxy** that sits between your application and AI APIs.
It classifies every request, routes simple tasks to cheap models and complex tasks to
premium ones, caches duplicate prompts, and enforces budget caps — all without changing
a single line of your code.

```
Your App ──→ TokenWise Core (:9401) ──→ Smart Router ──→ DeepSeek Chat  (cheap)
                                                      ──→ GPT-4.1 Mini  (mid)
                                                      ──→ Claude Sonnet  (premium)
                                    │
                                    └── Dashboard (:9400) — real-time cost tracking
```

---

## Architecture

TokenWise Core runs as a **single binary** with two ports:

| Port | Role |
|------|------|
| `9401` | **Proxy** — your app sends requests here. Forwards to AI APIs, applies routing + caching. |
| `9400` | **Dashboard** — web UI for monitoring costs, filtering calls, and configuring providers. |

### Zero-Trust Design

TokenWise Core **never holds your API keys**. In pass-through mode, the client's
`Authorization` header is forwarded directly to the AI provider — your key stays in
your application. The dashboard stores only metadata (model, tokens, cost, latency)
and SHA-256 hashes of your prompts for cache deduplication. Message content is
never written to disk.

```
Browser JS (localStorage) ──→ Proxy (:9401) ──→ AI API
   ↑                              ↑
   Key stored here            Key forwarded, never seen by admin server (:9400)
```

---

## Quick Start

### Option 1: Install Script

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/chentang1127-hub/tokenwise/main/install.sh | bash

# Windows (PowerShell as Admin)
irm https://raw.githubusercontent.com/chentang1127-hub/tokenwise/main/install.ps1 | iex
```

### Option 2: Docker

```bash
# Clone the repo (or download docker-compose.yml + Caddyfile + config.yaml)
git clone https://github.com/chentang1127-hub/tokenwise
cd tokenwise

# Edit config.yaml — add your API keys (or rely on client-side pass-through)
# Start with auto-TLS via Caddy (optional: set domain env vars)
docker compose up -d

# Dashboard → https://your-domain.com  (or http://localhost:9400)
# Proxy    → http://localhost:9401/v1

# Check status
curl http://localhost:9400/health
```

Set up a custom domain with Let's Encrypt:
```bash
CADDY_DASHBOARD_DOMAIN=dashboard.example.com \
CADDY_PROXY_DOMAIN=proxy.example.com \
  docker compose up -d
```

### Option 3: Build from Source

```bash
git clone https://github.com/chentang1127-hub/tokenwise
cd tokenwise
cargo build --release
./target/release/tokenwise start
```

### Point Your App

```bash
# Instead of:
export OPENAI_BASE_URL="https://api.deepseek.com/v1"

# Use:
export OPENAI_BASE_URL="http://localhost:9401/v1"
```

That's it. Every call is now tracked, routed, and optimized.

---

## Features

### Smart Routing
Simple tasks (translation, summarization, classification) are automatically routed
to the cheapest capable model. Complex tasks (code generation, reasoning, debugging)
go to premium models. Configurable via keyword lists and token thresholds.

| Complexity | Examples | Routed To |
|---|---|---|
| **Simple** | Translate, summarize, classify, "what is" | Cheapest tier model |
| **Medium** | Chat, general Q&A | Mid-tier model |
| **Complex** | Debug, implement, design, step-by-step | Premium tier model |

### Response Cache (Pro)
Identical prompts within 24 hours are served from cache with **$0 API cost**.
SHA-256 hashing ensures privacy while enabling exact-match deduplication.

**Semantic Cache** (Pro) extends this with Jaccard word-overlap matching —
paraphrases and similar prompts also hit the cache, not just exact duplicates.
Configurable similarity threshold.

### Anthropic Messages API (Native)
TokenWise Core translates Anthropic Messages API format (`/v1/messages`) to OpenAI
format internally, so Claude Code, Claude API SDKs, and any Anthropic-native client
can route through the proxy. Streaming SSE events are translated in real-time.

```
# Anthropic format (auto-translated to OpenAI internally)
POST /v1/messages
POST /v1/deepseek/messages

# OpenAI format
POST /v1/chat/completions
```

### Path-Based Routing
Force requests through a specific provider by URL path:

```
# Smart routing (auto-select best model)
POST /v1/chat/completions

# Force a specific provider
POST /v1/openrouter/chat/completions
POST /v1/deepseek/chat/completions
POST /v1/openai/chat/completions
```

### Budget Caps
Set daily or monthly spending limits. TokenWise Core blocks requests with HTTP 429
when the cap is exceeded.

```yaml
budget:
  daily_limit_usd: 10.0    # Block after $10/day
  monthly_limit_usd: 200.0  # Block after $200/month
```

### Prometheus Metrics
`GET /metrics` on the admin port exposes Prometheus-compatible counters:
requests, cache hits, routing decisions, token counts, and cost in USD.

### Claude Code History Import
Import your existing Claude Code call history from JSONL transcript files:

```bash
tokenwise import --source ~/.claude/projects
# Scans recursively, deduplicates by message ID, maps models to providers.
# All historical data appears on the dashboard.
```

### Webhook Notifications (Pro)
Fire HTTP callbacks to Slack/Discord/custom endpoints on budget warnings
(80% threshold), budget exceeded, and spending anomaly detection. Test
endpoint at `/api/test-webhook` for quick validation.

### Multi-User / Multi-Tenant
Each API key is hashed into a tenant ID. Dashboard data, budgets, and
cache entries are tenant-scoped. No raw keys stored — zero-trust preserved.

### Production Deployment
Production-ready deployment files included:
- **Docker Compose** with Caddy auto-TLS (Let's Encrypt) for HTTPS
- **systemd** service file with `ProtectSystem=strict` sandboxing
- **TW_\*** environment variable overrides for headless/CI/CD deployment
- **Backup** CLI: `tokenwise backup` (WAL checkpoint + timestamped copy)
- **Status** CLI: `tokenwise status` (health checks + version + DB info)
- **Health** endpoint: JSON response with version, DB connectivity, uptime

### gRPC Proxy Mode
Experimental gRPC proxy on port 9402 for AI services that expose gRPC
endpoints. Uses the same routing, recording, and cost tracking as the
REST proxy. Enable with `--features grpc`.

### API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/` | `GET` | Dashboard (HTML) — hero cost, story cards, SVG chart, request log, FAQ |
| `/calls` | `GET` | Calls page with filters & pagination (HTML) |
| `/savings` | `GET` | Savings analysis — cache + routing real savings (HTML) |
| `/setup` | `GET/POST` | Setup wizard — no-key onboarding (HTML) |
| `/health` | `GET` | Health check — JSON: `{"status":"ok","version":"0.5.0","db":"connected","uptime_seconds":3600}` |
| `/metrics` | `GET` | Prometheus metrics — requests, cache hits, routing, streaming safety counters |
| `/api/demo` | `POST` | Test endpoint — returns mock response, no API key needed |
| `/api/token-distribution` | `GET` | Token usage breakdown by model (JSON) |
| `/api/budget-status` | `GET` | Current spending vs budget caps (JSON) |
| `/api/test-webhook` | `POST` | Send a test notification to the configured webhook URL |

#### `GET /api/token-distribution`

Returns token usage and cost breakdown by model for the current month.

```json
[
  {
    "model": "deepseek/deepseek-chat",
    "call_count": 42,
    "prompt_tokens": 12500,
    "completion_tokens": 8300,
    "total_cost": 0.0125
  },
  {
    "model": "openai/gpt-4.1-mini",
    "call_count": 15,
    "prompt_tokens": 3200,
    "completion_tokens": 5100,
    "total_cost": 0.0084
  }
]
```

Sorted by `total_cost` descending, limited to top 10 models.

#### `GET /api/budget-status`

Returns current spending against configured budget caps (UTC day / calendar month).

```json
{
  "daily": {
    "spent": 3.245,
    "limit": 10.0,
    "pct": 32.45
  },
  "monthly": {
    "spent": 47.832,
    "limit": 200.0,
    "pct": 23.916
  }
}
```

`pct` is 0–100. `limit` of `0.0` means no cap (unlimited).

#### `GET /metrics`

Returns Prometheus text-exposition format metrics:

```
# HELP tokenwise_requests_total Total number of chat completion requests processed.
# TYPE tokenwise_requests_total counter
tokenwise_requests_total 142
# HELP tokenwise_cache_hits_total Total number of requests served from response cache.
# TYPE tokenwise_cache_hits_total counter
tokenwise_cache_hits_total 23
# HELP tokenwise_cost_usd_total Total estimated USD cost.
# TYPE tokenwise_cost_usd_total gauge
tokenwise_cost_usd_total 0.452300
# HELP tokenwise_cache_hit_ratio Ratio of cache hits to total requests.
# TYPE tokenwise_cache_hit_ratio gauge
tokenwise_cache_hit_ratio 0.1619
```

Compatible with Prometheus, Grafana, and any OpenMetrics scraper.

### Real-Time Dashboard (HTMX)
No JavaScript framework. Filter calls by time range (24h/7d/30d/All), task complexity,
and execution decision (eliminated/routed/direct). See token counts, latency, and
cost for every request.

---

## Supported Providers

| Provider | Cheap | Mid | Premium |
|---|---|---|---|
| **DeepSeek** | deepseek-chat | — | deepseek-reasoner |
| **OpenAI** | gpt-4.1-nano | gpt-4.1-mini | gpt-4.1 |
| **Anthropic** | — | claude-haiku-4-5 | claude-sonnet-4-6, claude-opus-4-8 |
| **Google** | gemini-2.5-flash | — | gemini-2.5-pro |
| **Mistral** | — | mistral-small, codestral | mistral-large |
| **xAI** | — | grok-4-mini | grok-4 |
| **Groq** | llama-4-scout | llama-4-maverick | — |
| **OpenRouter** | deepseek/deepseek-chat, google/gemini-2.5-flash | openai/gpt-4.1-mini, claude-haiku-4-5 | claude-sonnet-4-6 |

Add custom providers by editing `config.yaml`. Any OpenAI-compatible API works —
Ollama, vLLM, LiteLLM, local models, custom endpoints.

---

## Configuration

Full `config.yaml` reference:

```yaml
locale: "en"              # "en" or "zh"
headless: false           # Set true for Docker/remote servers

proxy:
  listen: "0.0.0.0:9401"  # Proxy port (0.0.0.0 = all interfaces)
  admin: "0.0.0.0:9400"   # Dashboard port
  timeout_secs: 120

providers:                # Add/remove as needed
  - name: "deepseek"
    base_url: "https://api.deepseek.com/v1"
    api_key_env: "DEEPSEEK_API_KEY"
    models:
      - id: "deepseek-chat"
        tier: "cheap"
        cost_per_1k_prompt: 0.00027
        cost_per_1k_completion: 0.0011

routing:
  simple_keywords: ["summarize", "translate", "extract", ...]
  complex_keywords: ["debug", "implement", "refactor", ...]
  tier_simple: "cheap"
  tier_complex: "premium"
  tier_default: "mid"

safety_net:
  enabled: true
  fallback_map:
    cheap: "mid"
    mid: "premium"

cache:
  ttl_hours: 24
  max_entries: 10000

budget:
  daily_limit_usd: 0.0     # 0 = unlimited
  monthly_limit_usd: 0.0

storage:
  db_path: "./tokenwise.db"
  retention_days: 90
```

---

## CLI

```bash
tokenwise start                     # Start proxy + dashboard
tokenwise --config custom.yaml start # Custom config file
tokenwise --market cn start          # Chinese market (config.cn.yaml)
tokenwise validate                   # Validate config syntax
tokenwise import --source ./claude   # Import Claude Code JSONL call history
tokenwise keygen --days 365          # Generate Pro license key
tokenwise backup --output ./backups  # WAL checkpoint + timestamped DB copy
tokenwise status                     # Check proxy/dashboard health + version + DB size
tokenwise --help                     # Show all options
```

---

## Pricing

| Tier | Price | Features |
|---|---|---|
| **Free** (OSS) | $0 | All providers, passthrough mode, SQLite recording, savings diagnostics, budget caps |
| **Pro** | $29/mo or $290/yr | Smart routing, semantic cache, safety net fallback, token distribution, email support |
| **Enterprise** | Custom | Private deployment, SSO, audit logs, SLA |

[Buy Pro →](https://tokenwise.lemonsqueezy.com) (Lemon Squeezy)
[国内购买 →](https://afdian.com/a/tokenwise) (爱发电)

---

## Data Privacy

- **Everything stays on your machine.** TokenWise Core is a local binary. No data is
  ever sent to a third party except to the AI providers you configure.
- API keys live in **your browser's localStorage**, never on TokenWise's server.
- Message content is hashed (SHA-256) for cache matching only — plaintext is never
  written to disk.
- SQLite database at `tokenwise.db` with configurable retention. Delete it anytime.

---

## Development

```bash
cargo build              # Debug build
cargo test               # Run 48 tests (36 unit + 12 integration)
cargo build --release    # Release build
cargo clippy             # Lint check

# Tech Stack
# Proxy:     Hyper 1.x (minimal overhead)
# Admin:     Axum 0.8 (ergonomic routing)
# Templates: Askama 0.13 (compile-time safety)
# Dashboard: HTMX 2.0 (zero JS framework)
# Storage:   SQLite (bundled, WAL mode)
# HTTP:      reqwest 0.12 (rustls)
```

### Project Structure

```
src/
├── main.rs              # CLI entry + graceful shutdown
├── lib.rs               # Library root (re-exports all modules)
├── config.rs            # YAML config loader + model resolution
├── license.rs           # Pro license verification (HMAC-SHA256) + keygen
├── webhooks.rs          # HTTP webhook dispatcher (budget alerts, anomalies)
├── multi_user.rs        # Tenant ID derivation + scoping
├── grpc_proxy.rs        # gRPC proxy types + routing logic
├── proxy/
│   ├── server.rs        # Transparent proxy (routing, caching, budget, CORS, metrics)
│   ├── classifier.rs    # Rule-based complexity classifier
│   ├── router.rs        # Model selection + fallback chains
│   ├── anthropic_format.rs  # Anthropic Messages API ↔ OpenAI format translation
│   └── tee_stream.rs    # SSE stream tee (zero-latency hot path)
├── recording/
│   ├── model.rs         # CallRecord data model + SHA-256 hashing
│   └── store.rs         # SQLite queries, cache ops, stats aggregation
├── cache/
│   └── mod.rs           # Semantic cache (Jaccard word-overlap)
├── admin/
│   ├── mod.rs           # AppState + router builder
│   ├── api.rs           # Dashboard handlers, token distribution, budget API
│   ├── metrics.rs       # Prometheus metrics counters + /metrics endpoint
│   └── chat_widget.rs   # Inline "Try It" chat (zero-trust, browser JS)
├── cost/
│   └── calculator.rs    # Per-call cost computation + formatting
└── import.rs            # Claude Code JSONL history importer

templates/
├── dashboard.html       # EN dashboard (hero + cards + SVG chart + log + FAQ)
├── calls.html           # EN calls page (stats bar + filters + pagination)
├── savings.html         # EN savings (real cache + routing savings)
├── setup.html           # EN setup wizard
├── 404.html             # EN 404 error page (dark theme)
├── 500.html             # EN 500 error page
├── 429.html             # EN 429 rate-limit page
└── cn/                  # Chinese (zh-CN) equivalents of all 7 pages

deploy/
└── tokenwise.service    # systemd unit file
```

---

## FAQ

**Q: Does this support streaming (SSE)?**
Yes. TokenWise Core uses a non-blocking stream tee — chunks are forwarded immediately.
Usage data is extracted on a cold path with zero added latency.

**Q: What if the cheap model fails?**
The safety net auto-retries with the next tier. Only triggers on 5xx server errors,
not 4xx auth errors — your API key won't trigger unnecessary fallbacks.

**Q: Does this work with OpenAI SDKs?**
Yes. Any SDK that lets you set `base_url` works: OpenAI Python/Node, LangChain,
LlamaIndex, Vercel AI SDK, etc.

**Q: Can I use this with local models (Ollama, vLLM)?**
Yes — add them as providers in `config.yaml` with `base_url: "http://localhost:11434/v1"`.

**Q: How do I force a specific provider?**
Use path-based routing: `POST /v1/deepseek/chat/completions` forces DeepSeek.
`POST /v1/chat/completions` uses smart routing.

**Q: Is there latency overhead?**
< 1ms for classification + routing. Cache hits return in < 10ms.

**Q: Where is my data stored?**
Local SQLite only. Nothing leaves your machine. No telemetry, no analytics, no cloud.

---

TokenWise Core is [MIT licensed](LICENSE). Built with Rust. PRs welcome.
