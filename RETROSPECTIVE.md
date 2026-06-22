# TokenWise Core v0.2.0 项目复盘

> 2026-06-21 · 48 tests · 7,330 行源码

---

## 一、项目概览

| 指标 | 数值 |
|------|------|
| Rust 源码 | 6,057 行 (24 文件) |
| HTML 模板 | 1,273 行 (8 文件, 中英双语) |
| 单元测试 | 36 个 (7 个模块) |
| 集成测试 | 12 个 |
| 测试总计 | **48 个, 全部通过** |
| CLI 命令 | 4 个 (start / validate / import / keygen) |
| Rust 依赖 | 18 个 crate |
| 配置文件 | 2 个 (config.yaml + config.cn.yaml, 结构一致) |
| Git 提交 | 16 次 |

---

## 二、架构

```
Claude Code / 应用
       │
       │ Anthropic 格式 (/v1/messages)
       │ 或 OpenAI 格式 (/v1/chat/completions)
       ▼
┌──────────────────────────────────────┐
│         TokenWise Proxy (:9401)      │
│                                      │
│  • 格式翻译 (Anthropic ↔ OpenAI)     │
│  • 复杂度分类 (simple/mid/complex)   │
│  • 智能路由 (provider内选最便宜模型)  │
│  • 预算拦截 (日/月限额 → 429)        │
│  • 响应缓存 (SHA-256 exact + Jaccard) │
│  • SSE 流式透传 + token 计数        │
└──────────────────────────────────────┘
       │
       │ OpenAI 格式
       ▼
┌──────────────┐    ┌──────────────────┐
│ DeepSeek API │... │ 其他 7 个 providers│
└──────────────┘    └──────────────────┘
       │
       ▼
┌──────────────────────────────────────┐
│      Admin Dashboard (:9400)         │
│                                      │
│  • 实时仪表板 (HTMX + SVG 图表)      │
│  • 调用记录 (筛选 + 分页)            │
│  • Prometheus /metrics               │
│  • 中英文切换                        │
│  • Try It 聊天测试                   │
│  • 信任 FAQ                          │
└──────────────────────────────────────┘
              │
              ▼
       SQLite (tokenwise.db)
       本地存储, 离线可用
```

---

## 三、已完成功能清单

### P0 — 核心能力 ✅
- [x] OpenAI 兼容代理 (`/v1/chat/completions`)
- [x] **Anthropic Messages API 代理** (`/v1/messages`) — Claude Code 原生格式支持
- [x] 路径强制 provider: `/v1/{provider}/chat/completions`, `/v1/{provider}/messages`
- [x] **格式双向翻译** — Anthropic ↔ OpenAI 请求/响应/SSE 全覆盖
- [x] 复杂度分类器 (关键词 + token 阈值, 中英文)
- [x] License 验证 (HMAC-SHA256, Free/Pro 分层)
- [x] 真实 LICENSE_SECRET (编译时常量)

### P1 — 仪表板 + 可观测性 ✅
- [x] Hero 数字 (本月费用)
- [x] Story Cards (缓存命中/智能路由/模型追踪)
- [x] SVG 支出分布图 (按模型着色)
- [x] 请求日志 (终端风格, 含状态标签)
- [x] Pro Savings 真实计算 (不再 5x 硬编码)
- [x] Try It 折叠组件 (localStorage 存 Key)
- [x] 信任 FAQ (Key 在哪/存什么/谁能看/数据在哪)
- [x] 中英文双语仪表板
- [x] Calls 页面 (筛选 + 分页 + 摘要统计)
- [x] Savings 页面

### P2 — 高级特性 ✅
- [x] **响应缓存**: SHA-256 精确匹配 + Jaccard 语义相似 (0.85 阈值)
- [x] **Prometheus /metrics**: 6 计数器 + cache_hit_ratio gauge
- [x] **集成测试**: 12 个 (用 axum oneshot, 无 TCP)
- [x] **Claude Code 历史导入**: 从 JSONL 转录文件导入 6,761 次调用
- [x] Pro License 生成器 (`tokenwise keygen`)

### P3 — 基础建设 ✅
- [x] Webhook 通知框架 (预算告警 + 异常检测)
- [x] 多租户基础 (TenantId = SHA-256 of API key)
- [x] gRPC 代理类型定义和架构设计

---

## 四、仪表板实测数据

| 模型 | 调用次数 | Prompt Tokens | Completion Tokens | 费用 (USD) |
|------|---------|---------------|-------------------|-----------|
| deepseek-v4-pro | 6,428 | 7,534,621 | 3,163,048 | $5.51 |
| deepseek-v4-flash | 134 | 613,489 | 747 | $0.17 |
| deepseek-reasoner | 1 | 18 | 2,332 | $0.005 |
| deepseek-chat | 11 | 97 | 3,236 | $0.004 |
| 其他 (demo/synthetic) | 48 | — | — | $0.00 |
| **合计** | **6,622** | — | — | **$5.69** |

---

## 五、待解决问题

### 高优先级

1. **Webhook 未接入** — `webhooks.rs` 定义了完整的预算告警状态机 + 异常检测，但 `Config` 无 webhook 字段，服务未调用。**改动量**: 10 行 config + 5 行 main.rs。

2. **多租户未接入** — `multi_user.rs` 的 `TenantId`/`TenantContext` 仅定义未使用，所有数据归入隐式匿名租户。

3. **gRPC 不可编译** — `grpc_proxy.rs` 文档说 `--features grpc` 但 `Cargo.toml` 无 `[features]`，`tonic`/`prost` 依赖缺失。

### 中优先级

4. **SafetyNet 未完全实现** — `fallback_on_empty_response` 和 `fallback_on_truncated` 只有 TODO 注释
5. **README 测试数量过时** — 写 21 个实际 48 个
6. **README CLI 不完整** — 缺 `import` 和 `keygen` 命令文档
7. **Anthropic 流式 token 计数不精确** — 当前用 prompt 字符数 ÷ 4 估算

### 低优先级

8. `webhooks.rs:78` `last_anomaly` 字段未读警告
9. README 项目结构中 `license.rs` 重复列出
10. `config.yaml` 含真实 Pro key 被提交 (取决于意图)

---

## 六、关键决策回顾

1. **Anthropic 格式支持采用翻译层而非双协议**: 内部统一用 OpenAI 格式处理，在入口/出口做格式翻译。优点：复用全部路由/缓存/分类逻辑。缺点：翻译层需要维护两份格式的字段映射。

2. **Jaccard 语义缓存代替 ONNX embedding**: 用词重叠度做近似语义匹配，零外部依赖，7 个测试覆盖。ONNX 方案留作未来升级路径。

3. **axum oneshot 做集成测试**: 不绑真实 TCP 端口，12 个测试 < 0.1s 完成，无端口冲突。

4. **SQLite WAL 模式**: 读写并发 + 优雅关闭时 checkpoint，数据安全有保障。

5. **Free tier passthrough 模式**: 客户端 API key 直传上游，TokenWise 自身不需要任何 key。零信任架构。

---

## 七、让它真正跑起来的最后一步

当前 Claude Code 的 `ANTHROPIC_BASE_URL` 仍指向 DeepSeek 直连:
```json
// C:\Users\t\.claude\settings.json (现状)
"ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic"
```

改为:
```json
"ANTHROPIC_BASE_URL": "http://127.0.0.1:9401/v1"
```

**改后重启 Claude Code**，所有 API 调用就会经过 TokenWise 代理，仪表板实时显示。TokenWise 保持运行即可:
```bash
cd C:\Users\t\tokenwise
cargo run --release -- start
# 或直接运行: .\target\release\tokenwise.exe start
```
