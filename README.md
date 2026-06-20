# ⚡ TokenWise

**Save 70–90% on AI API costs. One binary, zero code changes.**

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.96%2B-orange.svg)](https://www.rust-lang.org)

---

TokenWise sits between your app and AI APIs. It analyzes every prompt, routes simple tasks to cheap models and complex tasks to premium ones. You change one environment variable. That's it.

```
Before:  Your App ──→ OpenAI GPT-4.1 ($0.010/1K)    💸 $500/mo
After:   Your App ──→ TokenWise ──→ deepseek-chat  ($0.0003/1K)  simple tasks
                                  └─→ qwen-max     ($0.0008/1K)  complex tasks
                         💰 $50-150/mo
```

## Quick Start

### 1. Download

```bash
# Homebrew (macOS / Linux)
brew tap chentang1127-hub/tap
brew install tokenwise
tokenwise start

# Or download manually:
# macOS ARM64 (Apple Silicon)
curl -fsSL https://github.com/chentang1127-hub/tokenwise/releases/latest/download/tokenwise-macos-arm64.tar.gz | tar xz
./tokenwise start

# macOS x86_64 / Linux x86_64
curl -fsSL https://github.com/chentang1127-hub/tokenwise/releases/latest/download/tokenwise-linux-amd64.tar.gz | tar xz
./tokenwise start

# Windows (PowerShell)
Invoke-WebRequest -Uri "https://github.com/chentang1127-hub/tokenwise/releases/latest/download/tokenwise-windows-amd64.zip" -OutFile tokenwise.zip
Expand-Archive tokenwise.zip
.\tokenwise\tokenwise.exe start

# Docker
docker run -p 9400:9400 -p 9401:9401 \
  -e DEEPSEEK_API_KEY=sk-... \
  ghcr.io/chentang1127-hub/tokenwise:latest

# Build from source
git clone https://github.com/chentang1127-hub/tokenwise
cd tokenwise
cargo build --release
```

### 2. Configure

```bash
# Set your API keys
export DEEPSEEK_API_KEY=sk-...
export ZHIPU_API_KEY=...
export DASHSCOPE_API_KEY=...   # Qwen / Tongyi
export MOONSHOT_API_KEY=...
# ... any provider you use
```

### 3. Point your app at TokenWise

```bash
# Instead of:
export OPENAI_BASE_URL="https://api.openai.com/v1"

# Use:
export OPENAI_BASE_URL="http://localhost:9401/v1"
```

### 4. Open Dashboard

```
http://localhost:9400
```

That's it. Your app now saves 70-90% on every API call.

## How It Works

```
┌─────────────┐     ┌──────────────────────────────────────┐     ┌──────────────┐
│  Your App   │────▶│          TokenWise Proxy              │────▶│  DeepSeek    │
│             │     │                                      │     │  (cheap)     │
│  /v1/chat/  │     │  ┌──────────┐   ┌────────────────┐  │     └──────────────┘
│  completions│     │  │Classifier│──▶│    Router      │  │
│             │     │  │          │   │                │  │     ┌──────────────┐
│             │     │  │ simple?  │   │ same tier?     │  │────▶│  Qwen-Max    │
│             │     │  │ medium?  │   │ cheapest model │  │     │  (premium)   │
│             │     │  │ complex? │   │ + fallback     │  │     └──────────────┘
│             │     │  └──────────┘   └────────────────┘  │
│             │     │                                      │
│             │     │  • SSE stream tee (zero latency)     │
│             │     │  • SQLite recording (local only)     │
│             │     │  • HTMX Dashboard (:9400)            │
│             │◀────│  • Safety net (auto-retry upgrade)   │
└─────────────┘     └──────────────────────────────────────┘
```

### Classification Rules

| Tier | Examples | Routed To |
|---|---|---|
| **Simple** | Translate, summarize, classify, extract, "what is" | Cheapest model |
| **Complex** | Debug, implement, design, step-by-step reasoning | Best model |
| **Default** | Everything else | Mid-tier model |

Configurable per-language. Chinese keywords (翻译, 总结, 调试, 实现...) included in `config.cn.yaml`.

### Safety Net

If a cheap model returns an error or empty response, TokenWise automatically retries with the next tier:

```
deepseek-chat (cheap) ──✗──▶ qwen-plus (mid) ──✗──▶ deepseek-reasoner (premium)
```

## Supported Providers

### Global Market (`config.yaml`)
| Provider | Cheap | Mid | Premium |
|---|---|---|---|
| OpenAI | — | gpt-4o-mini | gpt-4.1 |
| DeepSeek | deepseek-chat | — | deepseek-reasoner |
| Anthropic | — | claude-haiku-4-5 | claude-sonnet-4-6 |

### China Market (`config.cn.yaml`, `--market cn`)
| Provider (厂商) | Cheap | Mid | Premium |
|---|---|---|---|
| DeepSeek 深度求索 | deepseek-chat | — | deepseek-reasoner |
| Zhipu 智谱 | glm-4-flash | glm-4-air | glm-4-plus |
| Qwen 通义千问 | qwen-turbo | qwen-plus | qwen-max |
| Moonshot 月之暗面 | — | moonshot-v1-8k | moonshot-v1-32k |
| Doubao 字节豆包 | doubao-lite-4k | doubao-pro-4k | doubao-pro-32k |
| Baidu 百度 | ernie-speed | ernie-lite | ernie-4.0 |
| MiniMax | abab6.5s | abab6.5 | — |

Add your own providers in `config.yaml` — it's just YAML.

## CLI

```bash
tokenwise start                          # Start proxy + dashboard (default config.yaml)
tokenwise --market cn start              # Start with Chinese market config
tokenwise --config my-config.yaml start  # Start with custom config
tokenwise validate                       # Check config syntax
tokenwise --market cn validate           # Check Chinese market config
tokenwise --help                         # Show all options
```

## Pricing

| Tier | Price | Features |
|---|---|---|
| **Free** (OSS) | $0 | Up to 3 providers, rule-based routing, SQLite recording |
| **Pro** | $29/mo or $290/yr | Unlimited providers, cache hit, ONNX embeddings, email support |
| **Enterprise** | Custom | Private deployment, SSO, audit logs, SLA |

[Buy Pro →](https://tokenwise.lemonsqueezy.com) (Lemon Squeezy)
[国内购买 →](https://afdian.com/a/tokenwise) (爱发电)

## Data Privacy

- **Everything stays on your machine.** TokenWise is a local proxy — no data is ever sent anywhere except to the AI providers you configure.
- SQLite database at `./tokenwise.db`, 90-day retention.
- Dashboard at `127.0.0.1:9400` — not exposed to the network.

## Development

```bash
# Build
cargo build

# Run tests
cargo test

# Release build (~12 MB)
cargo build --release

# Validate config
cargo run -- validate
cargo run -- --market cn validate
```

### Tech Stack

| Layer | Choice | Why |
|---|---|---|
| Proxy server | Hyper 1.x | Minimal overhead, HTTP/1.1 only |
| Admin API | Axum 0.8 | Ergonomic, Tower middleware |
| Templates | Askama 0.13 | Compile-time safety, zero runtime |
| Dashboard | HTMX 2.0 | No JS framework needed |
| Storage | SQLite (bundled) | Zero deps, WAL mode |
| HTTP client | reqwest 0.12 | Rustls, streaming support |
| Async | Tokio | Industry standard |

### Project Structure

```
src/
├── main.rs              # CLI entry point
├── config.rs            # YAML config loader
├── proxy/
│   ├── mod.rs           # Service builder
│   ├── server.rs        # Transparent proxy core
│   ├── classifier.rs    # Rule-based complexity classifier
│   ├── router.rs        # Model selection + fallback
│   └── tee_stream.rs    # SSE stream tee (zero-latency hot path)
├── recording/
│   ├── model.rs         # CallRecord data model
│   └── store.rs         # SQLite operations
├── admin/
│   ├── mod.rs           # App state
│   └── api.rs           # Dashboard API + HTMX handlers
└── cost/
    └── calculator.rs    # Cost computation + savings estimation

templates/
├── dashboard.html       # EN dashboard
├── calls.html           # EN calls page
├── savings.html         # EN savings page
└── cn/
    ├── dashboard.html   # CN dashboard (仪表板)
    ├── calls.html       # CN calls page (调用记录)
    └── savings.html     # CN savings page (节省分析)
```

## FAQ

**Q: Does this support streaming (SSE)?**
Yes. TokenWise uses a non-blocking stream tee — chunks are forwarded immediately, usage data is extracted on a cold path.

**Q: What if the cheap model fails?**
The safety net auto-retries with the next tier. Configurable in `config.yaml`.

**Q: Does this work with OpenAI SDKs?**
Yes. Any SDK that lets you set `base_url` (OpenAI Python, Node, LangChain, LlamaIndex, etc.) works.

**Q: Is there latency overhead?**
< 1ms for classification + routing. SSE tee adds zero latency on the hot path.

**Q: Can I use this with Ollama / local models?**
Yes — add them as providers in `config.yaml` with `base_url: "http://localhost:11434/v1"`.

**Q: Where is data stored?**
SQLite at the path in `storage.db_path`. Nothing leaves your machine. No telemetry.

---

TokenWise is [MIT licensed](LICENSE). Built with Rust. PRs welcome.
