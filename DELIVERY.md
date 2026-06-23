# TokenWise — 产品设计 & 交付逻辑

> 最后更新：2026-06-23

---

## 一、产品是什么

一句话：**一个透明代理，放在你的应用和 AI 厂商之间。不改代码，自动省钱、记账、兜底。**

```
你的应用 ──→ TokenWise (:9401) ──→ DeepSeek / OpenAI / Anthropic / ...
                  │
            Dashboard (:9400) — 看调用记录、费用、缓存命中
```

三层价值：

| 层级 | 功能 | 客户感知 |
|------|------|---------|
| 省钱 | 简单问题走便宜模型，缓存重复问题 | "账单少了 70%" |
| 可控 | 数据不离开自己服务器，零信任架构 | "我的 key 我自己管" |
| 可见 | Dashboard 看到每次调用、费用、路由决策 | "钱花哪了一目了然" |

### 零信任架构

TokenWise 不存你的 API key。请求发过来，Authorization header 直接转发给上游厂商。所以：

- Dashboard 能看到调了什么模型、花了多少钱
- 但永远看不到你的 API key
- 数据在你自己的服务器上（SQLite 文件）

---

## 二、客户怎么用

### 接入（唯一一步）

改一个环境变量或一行配置：

```
DeepSeek 用户：  ANTHROPIC_BASE_URL = http://你的服务器/v1
OpenAI 用户：   OPENAI_BASE_URL    = http://你的服务器/v1
```

### 出问题怎么办

改回去就恢复直连：

```
ANTHROPIC_BASE_URL = https://api.deepseek.com/anthropic   ← 改回这一行
```

10 秒恢复，不影响工作。

---

## 三、交付前检查清单

**每次发布前，以下全部通过才算 ready：**

### 端到端测试（curl 脚本）

```bash
# 1. 健康检查
curl http://llm.getipgeo.com/health

# 2. 非流式请求（Anthropic 格式）
curl http://llm.getipgeo.com/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"deepseek-v4-flash","max_tokens":50,"messages":[{"role":"user","content":"say hi"}],"stream":false}'

# 3. 流式请求（SSE）
curl -N http://llm.getipgeo.com/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"deepseek-v4-flash","max_tokens":50,"messages":[{"role":"user","content":"say hi"}],"stream":true}'

# 4. 错误场景：假 key
curl http://llm.getipgeo.com/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: fake-key" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"deepseek-v4-flash","max_tokens":10,"messages":[{"role":"user","content":"hi"}],"stream":false}'
# 预期：返回 4xx 错误，不是 502/空响应/崩溃

# 5. OpenAI 格式兼容
curl http://llm.getipgeo.com/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"hi"}],"max_tokens":10}'
```

### 稳定性验证

| 检查项 | 通过标准 |
|--------|---------|
| SSE 事件格式 | 每个事件间有 `\n\n` 空行分隔 |
| 非流式响应 | JSON 格式正确，有 content |
| 错误处理 | 假 key 返回 4xx，不崩 |
| 长时间运行 | 跑 1 小时，无 panic/内存泄漏 |
| Dashboard | 调用记录正确显示 |

---

## 四、不做的事

1. **不自动改客户配置** — 切不切、什么时候切，客户自己决定
2. **不碰客户正在用的 key** — key 永远只是转发，不落盘
3. **不给客户发"测试脚本"** — 测试是我的事，不是客户的事
4. **不发布未跑通检查清单的版本**

---

## 五、当前状态

| 项目 | 状态 |
|------|------|
| 核心功能 | ✅ v0.6.0 完成 |
| VPS 部署 | ✅ 运行中 |
| SSE 格式修复 | ✅ 已修已部署 |
| Claude Code 集成 | ⚠️ 你已切回直连，TokenWise 旁路运行 |
| 旁路/降级开关 | ❌ 待开发 |
| 自动化测试脚本 | ❌ 待开发 |

### 下一步

1. 写自动化测试脚本（上面的 curl 用例）
2. 加旁路开关（出问题时自动回退直连）
3. 跑通检查清单
4. 你再决定切不切
