# TokenWise — 文件地图（小白版）

> 不用懂 Rust 也能看懂每个文件干什么。
> 改功能时先看这里，就知道该动哪个文件。

---

## 📂 项目总览

```
tokenwise/
├── src/                    ← 所有 Rust 源代码在这
│   ├── main.rs             ← 🚪 程序入口（启动、命令行）
│   ├── lib.rs              ← 📋 模块目录
│   ├── config.rs           ← ⚙️ 配置读取
│   ├── license.rs          ← 🔑 Pro 许可证验证
│   ├── import.rs           ← 📥 导入 Claude Code 历史
│   ├── webhooks.rs         ← 🔔 通知（预算告警等）
│   ├── multi_user.rs       ← 👥 多用户租户
│   ├── grpc_proxy.rs       ← 🔧 gRPC 代理（实验性）
│   │
│   ├── proxy/              ← 🧠 核心：代理服务器
│   │   ├── server.rs       ←    🔥 主逻辑入口（请求→处理→响应）
│   │   ├── anthropic_format.rs ← 🔄 Anthropic↔OpenAI 格式翻译
│   │   ├── classifier.rs   ←    🏷️ 复杂度分类（简单/中等/复杂）
│   │   ├── router.rs       ←    🧭 智能路由（选最便宜模型）
│   │   └── tee_stream.rs   ←    📡 流式响应分叉
│   │
│   ├── recording/          ← 💾 数据存储
│   │   ├── model.rs        ←    📊 数据结构定义
│   │   └── store.rs        ←    🗄️ SQLite 数据库读写
│   │
│   ├── cache/              ← ⚡ 缓存
│   │   └── mod.rs          ←    语义缓存（重复问题不重复花钱）
│   │
│   ├── admin/              ← 🖥️ Dashboard 网页面板
│   │   ├── mod.rs          ←    服务器启动 + 路由
│   │   ├── api.rs          ←    各个页面的数据处理
│   │   ├── chat_widget.rs  ←    💬 Try It 聊天组件
│   │   └── metrics.rs      ←    📈 Prometheus 监控指标
│   │
│   └── cost/               ← 💰 费用计算
│       └── calculator.rs   ←    单次调用花费多少钱
│
├── templates/              ← 🎨 网页模板（HTML）
│   ├── dashboard.html      ←    📊 仪表板主页（英文）
│   ├── calls.html          ←    📋 调用记录页（英文）
│   ├── savings.html        ←    💵 节省分析页（英文）
│   ├── setup.html          ←    🧙 设置向导（英文）
│   ├── 404.html            ←    ❌ 404 错误页（英文）
│   ├── 500.html            ←    💥 500 错误页（英文）
│   ├── 429.html            ←    🚫 429 限流页（英文）
│   └── cn/                 ←    🇨🇳 中文版（以上 7 个页面都有）
│
├── deploy/                 ← 🚀 部署相关
│   └── tokenwise.service   ←    systemd 服务文件（Linux）
│
├── docs/                   ← 📝 社区发布材料
│   ├── reddit.md           ←    Reddit 帖子
│   ├── show-hn.md          ←    Hacker News 帖子
│   └── v2ex.md             ←    V2EX 帖子
│
├── tests/                  ← 🧪 集成测试
│   └── integration_tests.rs
│
├── Cargo.toml              ← 📦 Rust 依赖清单
├── Cargo.lock              ← 🔒 依赖版本锁定
├── config.yaml             ← ⚙️ 配置文件（英文/国际）
├── config.cn.yaml          ← ⚙️ 配置文件（中文/中国）
├── Dockerfile              ← 🐳 Docker 镜像构建
├── docker-compose.yml      ← 🐳 Docker 一键部署
├── Caddyfile               ← 🌐 HTTPS 反向代理配置
├── install.sh              ← 📥 Linux/macOS 安装脚本
├── install.ps1             ← 📥 Windows 安装脚本
├── .env.example            ← 🔐 环境变量参考
│
├── README.md               ← 📖 项目对外说明
├── TODO.md                 ← ✅ 当前进度 & 待办（你现在看的那个）
├── STRUCTURE.md            ← 🗺️ 本文件：文件地图
├── PLAN.md                 ← 📋 产品战略 & 阶段计划
├── PROJECT_STATUS.md       ← 📊 详细状态报告（功能清单、架构图）
└── RETROSPECTIVE.md        ← 📝 v0.2.0 项目复盘
```

---

## 🎯 改某个功能 → 应该看哪个文件？

| 你想做什么 | 涉及的主要文件 |
|-----------|--------------|
| 改 Dashboard 页面布局 | `templates/dashboard.html` + `templates/cn/dashboard.html` |
| 改 Calls 页面（调用记录） | `templates/calls.html` + `src/admin/api.rs` |
| 改 Savings 页面 | `templates/savings.html` + `src/admin/api.rs` |
| 加/改 AI 厂商配置 | `config.yaml` + `config.cn.yaml` + `src/config.rs` |
| 改路由规则（什么算简单/复杂） | `src/proxy/classifier.rs` + `config.yaml` 里的关键词 |
| 改智能路由逻辑 | `src/proxy/router.rs` |
| 改缓存策略 | `src/cache/mod.rs` |
| 改格式翻译 | `src/proxy/anthropic_format.rs` |
| 改数据库存储结构 | `src/recording/model.rs` + `src/recording/store.rs` |
| 改费用计算 | `src/cost/calculator.rs` |
| 改 Try It 聊天组件 | `src/admin/chat_widget.rs` |
| 改 Prometheus 指标 | `src/admin/metrics.rs` |
| 改命令行参数 | `src/main.rs` |
| 改 Docker 部署 | `Dockerfile` + `docker-compose.yml` + `Caddyfile` |
| 改安装脚本 | `install.sh` + `install.ps1` |
| 加新功能通知 | `src/webhooks.rs` |

---

## 🧠 核心流程（简化版）

```
用户请求到达
    │
    ▼
main.rs 启动两个服务 ──→ proxy (:9401)   ← 处理 API 请求
    │                   ──→ dashboard (:9400) ← 显示网页
    │
    ▼
proxy/server.rs 收到请求
    │
    ├─ 1. 格式翻译 (anthropic_format.rs)
    │      把 Anthropic 格式统一转成 OpenAI 格式
    │
    ├─ 2. 预算检查 (config.rs)
    │      今天/这个月花超了吗？→ 超了返回 429
    │
    ├─ 3. 复杂度分类 (classifier.rs)
    │      这个问题简单还是复杂？
    │
    ├─ 4. 智能路由 (router.rs)
    │      简单问题 → 便宜模型，复杂问题 → 强模型
    │
    ├─ 5. 缓存查询 (cache/mod.rs)
    │      这个问题之前问过吗？→ 问过就直接返回
    │
    ├─ 6. 发送给 AI 厂商
    │      流式? → tee_stream.rs 同时记录+透传
    │
    ├─ 7. 记录到数据库 (recording/store.rs)
    │      模型、token 数、费用、延迟
    │
    └─ 8. 返回给用户
```

---

## 📖 建议阅读顺序（如果你想学代码）

```
第1天：src/main.rs          → 看程序是怎么启动的
第2天：src/config.rs        → 看配置是怎么读的
第3天：src/proxy/server.rs  → 核心！看一个请求怎么被处理的
第4天：src/proxy/classifier.rs → 看怎么判断问题难不难
第5天：src/proxy/router.rs  → 看怎么选模型
第6天：src/recording/       → 看数据怎么存的
第7天：src/admin/api.rs     → 看仪表板数据怎么来的
```
