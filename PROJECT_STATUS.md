# TokenWise Core — 项目全景进度图

> 2026-06-22 · 51 tests (36 unit + 15 integration) · ~7,000 行 Rust · 14 HTML 模板 · P1+P2+P3+P4 全部完成

---

## 零、一句话定位

**自托管的 LLM 执行层** — 一个 Rust 二进制文件，零代码改动，接入你的应用和 AI 厂商之间。自动分类、路由、缓存、兜底、记账。

---

## 一、架构全景图

```
                    ┌──────────────────────────────────────────────┐
                    │              你的应用 / Claude Code            │
                    │  OpenAI SDK   Anthropic SDK   自定义 HTTP      │
                    └──────┬────────────┬──────────────┬───────────┘
                           │            │              │
              ┌────────────┼────────────┼──────────────┼────────────┐
              │            ▼            ▼              ▼            │
              │   POST /v1/chat/completions   POST /v1/messages     │
              │   POST /v1/deepseek/chat/completions                │
              │   POST /v1/deepseek/messages                        │
              │                                                    │
              │            ┌──────────────────────┐                │
              │            │   格式翻译层 (入口)     │                │
              │            │ Anthropic → OpenAI     │                │
              │            └──────────┬───────────┘                │
              │                       │                             │
              │            ┌──────────▼───────────┐                │
              │            │   复杂度分类器         │                │
              │            │ Simple / Mid / Complex│                │
              │            └──────────┬───────────┘                │
              │                       │                             │
              │            ┌──────────▼───────────┐                │
              │            │   智能路由器 (Pro)     │                │
              │            │ 同provider内最优模型   │                │
              │            └──────────┬───────────┘                │
              │                       │                             │
              │            ┌──────────▼───────────┐                │
              │            │   响应缓存 (Pro)      │                │
              │            │ SHA-256精确+Jaccard语义│               │
              │            └──────────┬───────────┘                │
              │                       │                             │
              │         ┌─────────────▼─────────────┐              │
              │         │     预算拦截               │              │
              │         │ 日/月超额 → HTTP 429       │              │
              │         └─────────────┬─────────────┘              │
              │                       │                             │
              │         ┌─────────────▼─────────────┐              │
              │         │   上游请求 + 安全兜底       │              │
              │         │ 5xx → fallback 模型        │              │
              │         │ 空响应/截断 → fallback      │              │
              │         └─────────────┬─────────────┘              │
              │                       │                             │
              │         ┌─────────────▼─────────────┐              │
              │         │   格式翻译层 (出口)         │              │
              │         │ OpenAI → Anthropic         │              │
              │         │ (SSE事件实时翻译)           │              │
              │         └─────────────┬─────────────┘              │
              │                       │                             │
              │         ┌─────────────▼─────────────┐              │
              │         │   录音 + 多租户标记         │              │
              │         │ CallRecord → SQLite        │              │
              │         └───────────────────────────┘              │
              │                                                    │
              │    TokenWise Proxy :9401                           │
              └──────────────────────┬─────────────────────────────┘
                                     │
                    ┌────────────────┼────────────────┐
                    ▼                ▼                 ▼
             ┌──────────┐   ┌──────────────┐   ┌──────────┐
             │ DeepSeek │   │ 其他7厂商      │   │ 本地模型  │
             │ API      │   │ OpenAI/Anth.. │   │ Ollama等  │
             └──────────┘   └──────────────┘   └──────────┘

┌──────────────────────────────────────────────────────────────────┐
│                    Admin Dashboard :9400                          │
│                                                                  │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐                       │
│  │ Hero 费用 │  │ 3 故事卡 │  │ SVG 图表 │                       │
│  │ $5.72    │  │ 缓存/路由 │  │ 按模型着 │                       │
│  │ 6,632次  │  │ /追踪模型│  │ 色分布   │                       │
│  └──────────┘  └──────────┘  └──────────┘                       │
│                                                                  │
│  ┌──────────────────────────────────────────┐                    │
│  │  请求日志 (model | 状态标签 | cost | 时间) │                    │
│  └──────────────────────────────────────────┘                    │
│                                                                  │
│  ┌────────────┐ ┌──────────────┐ ┌──────────┐                   │
│  │ ▶ Try It   │ │ 🔒 信任FAQ   │ │ 中/EN切换│                   │
│  │ (折叠聊天)│ │ (Key/数据/隐私)│ │           │                   │
│  └────────────┘ └──────────────┘ └──────────┘                   │
│                                                                  │
│  /calls (筛选+分页)  /savings (缓存+路由节省)  /metrics (Prom)   │
└──────────────────────────────────────────────────────────────────┘
```

---

## 二、文件清单 & 状态

### 🔵 核心引擎 (proxy/)

| 文件 | 行数 | 职责 | 状态 |
|------|------|------|------|
| `server.rs` | 1,028 | 请求入口、格式翻译调用、路由、缓存查/写、上游调用、兜底、录音 | ✅ 完成 |
| `anthropic_format.rs` | 736 | Anthropic↔OpenAI 双向翻译：请求转换、响应转换、SSE事件流实时翻译 | ✅ 完成 |
| `classifier.rs` | 334 | 中英文关键词+token阈值复杂度分类 (Simple/Mid/Complex) | ✅ 完成 |
| `router.rs` | 235 | 模型选择 + fallback链 (同provider优先，全局兜底) | ✅ 完成 |
| `tee_stream.rs` | 135 | SSE流分叉 — 热路径直接透传，冷路径提取token用量 | ✅ 完成 |
| `mod.rs` | 5 | 模块导出 | ✅ |

### 🟢 存储层 (recording/)

| 文件 | 行数 | 职责 | 状态 |
|------|------|------|------|
| `store.rs` | 629 | SQLite建表/迁移、CRUD、缓存、月度统计、筛选查询、路由/模型统计 | ✅ 完成 |
| `model.rs` | 79 | CallRecord 数据模型 + SHA-256哈希 + tenant_id | ✅ 完成 |
| `mod.rs` | 6 | 模块导出 | ✅ |

### 🟡 管理面板 (admin/)

| 文件 | 行数 | 职责 | 状态 |
|------|------|------|------|
| `api.rs` | 823 | Dashboard/Calls/Savings/Setup handler、模板渲染、Token分布API、预算API、中英双语 | ✅ 完成 |
| `chat_widget.rs` | 247 | 折叠式 Try It 聊天组件 (localStorage存Key，零信任) | ✅ 完成 |
| `metrics.rs` | 145 | Prometheus counters/gauges + /metrics端点 | ✅ 完成 |
| `mod.rs` | 25 | AppState + router构建 | ✅ |

### 🟣 基础设施

| 文件 | 行数 | 职责 | 状态 |
|------|------|------|------|
| `main.rs` | 369 | CLI (start/validate/import/keygen/backup/status)、优雅关闭、WAL checkpoint | ✅ P4 增强 |
| `config.rs` | 235 | YAML加载、模型查找、最便宜模型查询、webhook字段、TW_* env override | ✅ P3 增强 |
| `lib.rs` | 20 | 模块声明 | ✅ 完成 |

### 🟠 高级功能

| 文件 | 行数 | 职责 | 状态 |
|------|------|------|------|
| `cache/mod.rs` | 262 | Jaccard词重叠语义缓存 (0.85阈值) | ✅ 完成 |
| `license.rs` | 211 | HMAC-SHA256 Pro验证 + 编译时SECRET | ✅ 完成 |
| `import.rs` | 273 | Claude Code JSONL转录文件导入 | ✅ 完成 |
| `webhooks.rs` | 186 | Webhook通知框架 (预算告警+异常检测+冷却) | ⚠️ 框架就绪，未接入proxy |
| `multi_user.rs` | 72 | TenantId (API Key→SHA-256)、租户上下文 | ⚠️ 写入就绪，Dashboard未过滤 |
| `grpc_proxy.rs` | 130 | gRPC代理类型定义 | 🔧 不可编译 (缺tonic/prost) |
| `cost/calculator.rs` | 46 | 成本计算 + USD格式化 | ✅ 完成 |

### ⚙️ 部署 (P3)

| 文件 | 职责 | 状态 |
|------|------|------|
| `Dockerfile` | 多阶段构建 (rust:1.85-alpine → alpine:3.21)，非root用户 | ✅ |
| `docker-compose.yml` | tokenwise + Caddy (auto Let's Encrypt TLS)，健康检查，持久化 | ✅ |
| `Caddyfile` | 双虚拟主机 (dashboard + proxy)，gzip，安全 headers | ✅ |
| `deploy/tokenwise.service` | systemd 单元 (ProtectSystem=strict, Restart=on-failure) | ✅ |
| `.env.example` | 环境变量参考 (CADDY_*, TW_*, RUST_LOG) | ✅ |

### 📄 模板 (templates/)

| 文件 | 职责 | 状态 |
|------|------|------|
| `dashboard.html` | EN仪表板 (Hero+3卡片+SVG图表+日志+FAQ+动态版本) | ✅ |
| `calls.html` | EN调用记录 (统计条+筛选器+分页+动态版本) | ✅ |
| `savings.html` | EN节省分析 (缓存+路由真实计算+动态版本) | ✅ |
| `setup.html` | EN设置向导 + 动态版本 | ✅ |
| `404.html` | EN 404 错误页 (暗色主题) | ✅ P4 新增 |
| `500.html` | EN 500 错误页 (含排错提示) | ✅ P4 新增 |
| `429.html` | EN 429 限流页 (含预算提示) | ✅ P4 新增 |
| `cn/dashboard.html` | 中文仪表板 (完全翻译) | ✅ |
| `cn/calls.html` | 中文调用记录 | ✅ |
| `cn/savings.html` | 中文节省分析 | ✅ |
| `cn/setup.html` | 中文设置向导 | ✅ |
| `cn/404.html` | 中文 404 错误页 | ✅ P4 新增 |
| `cn/500.html` | 中文 500 错误页 | ✅ P4 新增 |
| `cn/429.html` | 中文 429 限流页 | ✅ P4 新增 |

---

## 三、功能完成度矩阵

| # | 功能 | 状态 | 测试覆盖 |
|---|------|------|----------|
| 1 | OpenAI 代理 (/v1/chat/completions) | ✅ 完成 | 集成测试 |
| 2 | Anthropic 代理 (/v1/messages) | ✅ 完成 | 6个单元测试 |
| 3 | 格式双向翻译 (请求+响应+SSE) | ✅ 完成 | 6个单元测试 |
| 4 | 路径强制provider | ✅ 完成 | — |
| 5 | 复杂度分类 (中英文) | ✅ 完成 | 8个单元测试 |
| 6 | 智能路由 (Pro) | ✅ 完成 | 4个单元测试 |
| 7 | 安全兜底 5xx | ✅ 完成 | — |
| 8 | 安全兜底 空响应 | ✅ 本周期新增 | — |
| 9 | 安全兜底 截断 | ✅ 本周期新增 | — |
| 10 | 响应缓存 SHA-256 精确 | ✅ 完成 | — |
| 11 | 响应缓存 Jaccard 语义 | ✅ 完成 | 7个单元测试 |
| 12 | 预算封顶 (日/月) | ✅ 完成 | 集成测试 |
| 13 | License 验证 (Pro/Free) | ✅ 完成 | 5个单元测试 |
| 14 | License 生成器 (keygen) | ✅ 完成 | — |
| 15 | Claude Code 历史导入 | ✅ 完成 | 2个单元测试 |
| 16 | Dashboard (Hero/卡片/图表/FAQ) | ✅ 完成 | 集成测试 |
| 17 | Calls 页面 (筛选+分页) | ✅ 完成 | 集成测试 |
| 18 | Savings 页面 (真实数字) | ✅ 完成 | 集成测试 |
| 19 | Prometheus /metrics | ✅ 完成 | 集成测试 |
| 20 | Try It 聊天组件 | ✅ 完成 | — |
| 21 | 中英双语 | ✅ 完成 | 集成测试 |
| 22 | 多租户 tenant_id 写入 | ✅ 完成 | — |
| 23 | 多租户 Dashboard 过滤 | ✅ 本周期新增 | — |
| 24 | Webhook 通知 proxy 接入 | ✅ 本周期新增 | — |
| 25 | Anthropic 流式精确 token | ✅ 本周期新增 | — |
| 26 | 流式安全兜底 (空/截断检测) | ✅ 本周期新增 | — |
| 27 | 清理真实 Pro key | ✅ 完成 | — |
| 28 | gRPC feature flag | ✅ 完成 | — |
| 29 | Dockerfile 多阶段构建 | ✅ P3 新增 | — |
| 30 | Docker Compose + Caddy TLS | ✅ P3 新增 | — |
| 31 | systemd 服务 | ✅ P3 新增 | — |
| 32 | TW_* 环境变量覆盖 | ✅ P3 新增 | — |
| 33 | Backup CLI 命令 | ✅ P3 新增 | — |
| 34 | Status CLI 命令 | ✅ P3 新增 | — |
| 35 | Health 端点增强 (JSON+DB+uptime) | ✅ P3 新增 | 集成测试 |
| 36 | 流式安全网 Prometheus 指标 | ✅ P3 新增 | — |
| 37 | Webhook 测试端点 | ✅ P4 新增 | — |
| 38 | 仪表板动态版本号 (全部模板) | ✅ P4 新增 | — |
| 39 | Try It 折叠 (默认收起) | ✅ P4 新增 | — |
| 40 | 错误页 404/500/429 (中英双语) | ✅ P4 新增 | — |

---

## 四、数据流全链路

```
HTTP Request 到达 :9401
  │
  ├─ 1. 路径解析 ──→ 判断 OpenAI/Anthropic，提取强制provider
  │
  ├─ 2. 预算检查 ──→ 日/月超限 → 429
  │
  ├─ 3. 提取认证 ──→ Authorization / x-api-key
  │                └─→ tenant_id = SHA-256(api_key)
  │
  ├─ 4. 格式翻译 ──→ Anthropic → OpenAI (统一内部格式)
  │
  ├─ 5. 复杂度分类 ──→ Simple / Mid / Complex
  │
  ├─ 6. 智能路由 ──→ 同provider内选最便宜对应tier模型
  │                │  或 Free模式直通
  │
  ├─ 7. 缓存查询 ──→ SHA-256精确匹配
  │                │  命中 → 直接返回 (0 API成本)
  │                │  未命中 → Jaccard语义匹配 (0.85)
  │                │  命中 → 直接返回
  │
  ├─ 8. 上游请求 ──→ POST {provider}/v1/chat/completions
  │                │  失败 (5xx/连接) → fallback模型重试
  │
  ├─ 9. 响应处理 ──→ 流式: SSE Tee (热路径透传+冷路径提取token)
  │                │  非流式: 读取body
  │                │  空响应/截断 → fallback模型重试
  │
  ├─ 10. 格式翻译(出) ──→ OpenAI → Anthropic (如需)
  │
  ├─ 11. 录音 ──→ CallRecord {
  │                  id, timestamp, model, provider, complexity,
  │                  prompt_tokens, completion_tokens, cost_usd,
  │                  latency_ms, fallback_used, was_routed,
  │                  tenant_id ← SHA-256(api_key),
  │                  prompt_hash ← SHA-256(messages)
  │              }
  │              ↓
  │              SQLite (WAL模式)
  │
  ├─ 12. 缓存写入 ──→ 非流式响应存入缓存 (24h TTL)
  │
  └─ 13. 返回响应 ──→ HTTP Response → 客户端
```

---

## 五、配置 & 支持矩阵

### AI 厂商 (EN config: 8家)

| Provider | Cheap | Mid | Premium |
|----------|-------|-----|---------|
| OpenAI | gpt-4.1-nano | gpt-4.1-mini | gpt-4.1 |
| Anthropic | — | claude-haiku-4-5 | claude-sonnet-4-6, claude-opus-4-8 |
| Google | gemini-2.5-flash | — | gemini-2.5-pro |
| DeepSeek | deepseek-chat, deepseek-v4-flash | — | deepseek-reasoner, deepseek-v4-pro |
| Mistral | — | mistral-small, codestral | mistral-large |
| xAI | — | grok-4-mini | grok-4 |
| Groq | llama-4-scout | llama-4-maverick | — |
| OpenRouter | gemini-2.5-flash, deepseek-chat | gpt-4.1-mini, haiku-4-5 | sonnet-4-6 |

### CN config 额外增加: 智谱/通义千问/月之暗面/豆包/百度/零一万物/MiniMax

### 协议支持

| 协议 | 端点 | 流式 | 非流式 | 格式翻译 |
|------|------|------|--------|----------|
| OpenAI | `/v1/chat/completions` | ✅ | ✅ | — |
| OpenAI+Provider | `/v1/{provider}/chat/completions` | ✅ | ✅ | — |
| Anthropic | `/v1/messages` | ✅ SSE | ✅ | ✅ 双向 |
| Anthropic+Provider | `/v1/{provider}/messages` | ✅ SSE | ✅ | ✅ 双向 |

---

## 六、仪表板实测数据

| 指标 | 数值 |
|------|------|
| 总调用次数 | 6,632 |
| 本月费用 | $5.72 |
| 最多调用模型 | deepseek-v4-pro (~6,433次) |
| 缓存命中 | 视Pro状态 |
| 路由调用 | 视Pro状态 |

---

## 七、剩余任务优先级

### 🔴 P0 — 无 (核心引擎已完备)

### 🟡 P1 — ✅ 全部完成

| # | 任务 | 状态 |
|---|------|------|
| 1 | Webhook 接入 proxy 请求流 | ✅ |
| 2 | Dashboard 多租户过滤 (`?tenant=`) | ✅ |

### 🟢 P2 — ✅ 全部完成

| # | 任务 | 状态 |
|---|------|------|
| 3 | gRPC 可编译 (feature flag) | ✅ |
| 4 | 流式安全兜底 (空/截断检测+告警) | ✅ |
| 5 | Anthropic 流式精确 token 计数 | ✅ |
| 6 | 清理 config.yaml 真实 Pro key | ✅ |

---

## 八、CLI 命令参考

```bash
tokenwise start                     # 启动 proxy(:9401) + dashboard(:9400)
tokenwise --config config.cn.yaml start  # 中国版配置
tokenwise --market cn start         # 同上，简写
tokenwise validate                  # 验证配置文件语法
tokenwise import --source ~/.claude/projects  # 导入 Claude Code 历史
tokenwise keygen --days 365         # 生成 Pro License Key
tokenwise backup --output ./backups # WAL checkpoint + 复制数据库
tokenwise status                    # 检查 proxy/dashboard 运行状态
```

---

## 九、开发命令

```bash
cargo build              # 调试构建
cargo build --release    # 发布构建 (~12 MB)
cargo test               # 48 测试 (36 单元 + 12 集成)
cargo clippy             # Lint 检查
cargo fmt                # 格式化
```

---

## 十、项目演进时间线

```
v0.1.0  初始提交 (proxy + dashboard 骨架)
  │
  ├─ 费用计算修正 (model字段改写)
  ├─ 社区发布材料 (Show HN/Reddit/V2EX)
  ├─ CI/CD (GitHub Actions 4平台构建)
  ├─ Dashboard 改进
  │
v0.2.0  核心功能完备
  │
  ├─ Free/Pro 分层 (直通 vs 智能路由)
  ├─ 响应缓存 (SHA-256 + Jaccard)
  ├─ 分类器增强 (中文/代码/多步推理检测)
  ├─ Anthropic API 完整支持 + 格式翻译
  ├─ Claude Code 历史导入 (6,761条)
  ├─ Dashboard 重构 (Hero+卡片+图表+FAQ+Try It)
  ├─ 安全兜底增强 (空响应+截断检测)
  ├─ 多租户 tenant_id 写入
  ├─ Webhook 框架就绪
  │
v0.3.0  ✅ P1+P2 全部完成
  │
  ├─ Webhook 接入 proxy 请求流
  ├─ Dashboard 多租户过滤
  ├─ gRPC 可编译
  ├─ 流式安全兜底 (空/截断检测+Prometheus指标)
  ├─ Anthropic 流式精确 token 计数
  │
v0.4.0  ✅ P3 部署就绪
  │
  ├─ Dockerfile 多阶段构建 (rust:1.85-alpine → alpine:3.21)
  ├─ Docker Compose + Caddy auto Let's Encrypt TLS
  ├─ systemd 服务文件 (ProtectSystem=strict)
  ├─ TW_* 环境变量覆盖配置
  ├─ Backup CLI (WAL checkpoint + 时间戳副本)
  ├─ Status CLI (HTTP 健康检查)
  ├─ Health 端点增强 (JSON+DB连接+uptime)
  ├─ 流式安全网 Prometheus 计数器
  │
v0.5.0  ✅ P4 打磨
  │
  ├─ Dashboard 重构 (Hero月费+3故事卡+FAQ)
  ├─ 真实 Savings 计算 (缓存估算+路由估算，无5x占位符)
  ├─ 动态版本号 (全部8个页面+6个错误页模板)
  ├─ Try It 默认折叠 (details/summary)
  ├─ 错误页 404/500/429 (中英双语，暗色主题)
  ├─ Webhook 测试端点 (POST /api/test-webhook)
  └─ Savings 页面真实数据驱动
```

---

**当前版本: v0.5.0 · 51 tests (36 unit + 15 integration) · 0 warnings · P1+P2+P3+P4 全部完成 · 生产可用**

### 版本历史

| 版本 | 里程碑 |
|------|--------|
| v0.1.0 | 初始 proxy + dashboard 骨架 |
| v0.2.0 | Free/Pro 分层、缓存、Anthropic 支持、导入 |
| v0.3.0 | Webhook 接入、多租户过滤、gRPC、流式安全兜底 |
| v0.4.0 | Docker 部署、systemd、env override、Backup/Status CLI |
| v0.5.0 | Dashboard 重构、真实 Savings、动态版本、错误页、Try It 折叠 |
