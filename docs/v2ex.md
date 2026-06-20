# TokenWise — 一个透明代理，帮你节省 70-90% AI API 费用

## 痛点

你接了一堆 AI API：OpenAI、Claude、DeepSeek、通义千问……但每次请求都调同一个模型。问"巴黎首都是哪"也走 GPT-4，debug 复杂代码也走 GPT-4。月账单 $500+。

## TokenWise 做了什么

一个 Rust 写的透明代理，跑在你本地。你的 app 配置一个环境变量指向它，然后：

```
简单问题 ("什么是 X") → deepseek-chat ($0.0003/1K)
中等任务 ("解释原理")   → qwen-max   ($0.0008/1K)
复杂任务 ("一步步 debug") → deepseek-reasoner / Claude
```

你的 app 不知道这些 — 它只看到 `OPENAI_BASE_URL=http://localhost:9401/v1`，TokenWise 在后面自动分类、路由、替换模型名。

## 技术实现

- **分类器:** 关键词 + 字符长度 + token 数估算，分析每个 prompt 的复杂度
- **路由器:** 配置文件定义 tier（cheap/mid/premium），自动匹配最便宜可用模型
- **安全网:** 上游挂掉自动 fallback 到下一级模型
- **录制:** SQLite 本地存储，HTMX 面板实时查看每笔调用和成本
- **模型名重写:** 客户端请求 `gpt-4o-mini` → TokenWise 替换为 `deepseek-chat`，客户端无感

## 支持的中国厂商

DeepSeek、智谱 GLM、通义千问、Moonshot/Kimi、豆包、百度文心、MiniMax

```
tokenwise --market cn start    # 中文配置，7 家中国厂商
```

## 数据

- 全部本地 — SQLite 存调用记录，不需要任何外部服务
- 零依赖 — 一个 16MB 二进制文件
- 开源 MIT

## 试一下

```bash
# macOS / Linux
brew tap chentang1127-hub/tap
brew install tokenwise

# 或直接下载
curl -fsSL https://github.com/chentang1127-hub/tokenwise/releases/latest/download/tokenwise-linux-amd64.tar.gz | tar xz

# 配置 API key
export DEEPSEEK_API_KEY=sk-...
export DASHSCOPE_API_KEY=...   # 通义千问

# 启动
tokenwise start
# 代理: http://127.0.0.1:9401
# 面板: http://127.0.0.1:9400
```

GitHub: https://github.com/chentang1127-hub/tokenwise
License: MIT，免费版最多 3 个 provider

欢迎提 Issue 和 PR。你们公司现在 AI API 月费多少？
