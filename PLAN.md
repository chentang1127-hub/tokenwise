# TokenWise 产品更新计划

基于 2026-06-20 竞品分析（TokenWise HQ）及策略讨论。

---

## 一、战略定位修正

### 旧定位
> "帮你省 LLM API 费用的工具"

归类：billing optimizer / cost tool
天花板：省钱面板

### 新定位
> "Self-hosted execution layer for LLM applications."

归类：infrastructure primitive
天花板：开发者基础设施

### 一句话
```
TokenWise 不是帮你省 tokens 的工具，
而是帮你决定每一次 LLM 请求应该如何被执行的系统。
```

---

## 二、竞争格局

| | TokenWise HQ | TokenWise (我们) |
|---|---|---|
| 形态 | SaaS 云端代理 | 自托管本地二进制 |
| 信任模型 | 必须信任 SaaS | 零信任，内容从不落盘 |
| 数据流向 | 你 → 他们云端 | 你 → 你的机器（闭环） |
| 接入方式 | `npx` SDK wrapper | `OPENAI_BASE_URL` 环境变量 |
| 定价 | 按月订阅 ($9.50-39.50/mo) | Pro 一次性买断 |
| 请求上限 | 有 (200K-2M/月) | 无限制 |
| 数据留存 | 60-180天 | 永久（本地 SQLite） |
| Prompt 压缩 | ✅ | ❌ |
| A/B 流量分割 | ✅ | ❌ |
| 质量自动回滚 | ✅ | ❌ |
| 告警 | ✅ | ❌ |
| 开源可审计 | ❌ | ✅ |
| 离线可用 | ❌ | ✅ |

### 核心差异：信任边界

他们强在功能广度，我们强在信任模型。
不是"谁功能更强"，是 **managed optimization service vs execution infrastructure primitive**。

他们的致命弱点：**功能越多（prompt trimming、payload inspection），信任越薄。**
我们的护城河：**不碰数据 = 可以进入他们永远进不去的市场（企业合规、GDPR、SOC2）。**

---

## 三、目标用户

| 用户 | 为什么选我们 | 为什么不用 SaaS |
|------|-------------|----------------|
| 企业/合规团队 | 数据不能离开服务器 | 法律不允许 |
| AI infra builder | 需要可控的执行链 | SaaS 不够底层 |
| Power indie dev | 不想被 lock-in | 信任偏好 + 预算 |

---

## 四、分阶段执行计划

### Phase 0：叙事对齐（今天 — 已完成 ✅）

- [x] Hero 重写：`Lossless Cost Reduction` / `无损成本优化`
- [x] 故事卡片重写：`Requests Eliminated` / `执行决策` / `执行层`
- [x] 终端风格 Request Log 替代表格
- [x] 所有文案从"省钱"转到"执行"

---

### Phase 1：接入体验（1-2 天）

**目标：把"下载→启动→看到数据"压缩到 30 秒内。**

| 项 | 现状 | 改进 | 改动量 |
|----|------|------|--------|
| 安装 | 手动下载二进制 | 一行 curl 安装脚本 | 1 个 shell 脚本 |
| 空状态 | "暂无 API 调用，设置环境变量" | "Your workspace is ready. Send your first request." + 环境变量提示 | 2 个模板改 3 行 |
| 卡片零值 | `0 请求被消除` | `重复请求将被自动消除` — 展示能力而非零值 | 2 个模板改 6 行 |
| 时间筛选 | 只有"本月" | 加 24h/7d/30d 查询参数 | api.rs 加 20 行 |
| Token 列 | Calls 页缺 | 加 `prompt_tokens` + `completion_tokens` | templates + api.rs |

**具体改动：**

1. `install.sh` — 一键安装脚本
2. `dashboard.html` / `cn/dashboard.html` — 空状态 + 零值叙事
3. `calls.html` / `cn/calls.html` — 加 Token 列 + 时间筛选
4. `api.rs` — calls_page handler 支持 `?range=24h|7d|30d`

---

### Phase 2：Calls 页面升级（2-3 天）

**目标：对齐竞品的信息密度，展示我们的架构优势。**

| 项 | 改动 |
|----|------|
| 时间筛选器 | 24h / 7d / 30d / 90d tabs |
| Token 列 | `prompt_tokens` / `completion_tokens` |
| 决策标签 | `eliminated` / `routed → cheaper` / `direct` |
| 过滤器 | `?filter=cache` / `?filter=routed` |
| 统计摘要 | 页顶：总请求数、总 token、总费用（选中时间范围） |

---

### Phase 3：架构差异化（3-5 天）

**目标：让"零信任 + 自托管"从 FAQ 变成产品主线。**

| 项 | 说明 |
|----|------|
| Docker 镜像 | `docker run -p 9400:9400 -p 9401:9401 -v ./tokenwise.db:/data/tokenwise.db tokenwise` |
| 无头模式 | `tokenwise start --config config.yaml` — 不启动 Dashboard，纯代理模式 |
| 架构图 | ASCII diagram：`Your App → TokenWise Proxy Layer [Route / Cache / Execute] → LLMs` |
| 零信任说明 | 从 FAQ 提到 Hero 下方小字：`Zero data leaves your machine. Your keys are never seen.` |

---

### Phase 4：功能补齐（1-2 周）

| 优先级 | 功能 | 理由 |
|--------|------|------|
| P0 | OpenRouter provider | 瞬间覆盖 200+ 模型，零维护成本 |
| P0 | 路径式路由 `{proxy}/openai/v1` | 更 infra，用户一看就懂 |
| P1 | 预算上限 | Pro 功能，月度预算超了全走 cheap |
| P1 | 请求量统计摘要 | Calls 页顶部数字 |
| P2 | Token 分布分析 | 不存原文，只统计每段 message 的 token 占比 |
| P2 | 每周摘要邮件 | 去 tokenwisehq 的"每周洞察" |

---

### Phase 5：命名与发布

**命名问题必须解决。**

选项：
- A: 新名字（LLM Gate / ExecLayer / RouteAI / ControlPlane）
- B: TokenWise Core / TokenWise OS（加后缀）
- C: 保留 TokenWise + 强 rebrand

**推荐 B**：`TokenWise Core` — 保留已有认知，加 `Core` 暗示这是更底层的版本。

发布清单：
- [ ] README.md 用新叙事重写
- [ ] 安装脚本
- [ ] Docker Hub 发布
- [ ] 定价页面（Pro 一次性 vs SaaS 月付对比）

---

## 五、定价策略

| | TokenWise HQ | TokenWise Core |
|---|---|---|
| 免费版 | 7天试用 | Forever Free（基本路由+追踪） |
| 付费版 | $9.50-39.50/月 | $29-49 一次性 Pro |
| 高端 | 联系销售 | 企业定制 |

**Pro 一次性 vs 订阅对比是我们最强的转化武器。**
不需要写"省钱"，只需要写："$29 once vs $114/year"。

---

## 六、一句话定位（可直接用）

```
TokenWise Core is a self-hosted execution layer for LLM applications.
It automatically routes, reuses, and eliminates requests —
without your data ever leaving your machine.
```

## 七、当前状态

- 编译：✅ 0 warnings
- 测试：✅ 21 passed
- Dashboard 叙事：✅ Phase 0 已完成
- 下一步：Phase 1（安装脚本 + 空状态 + 时间筛选）
