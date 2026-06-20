# Show HN: TokenWise — Save 70-90% on AI API costs with one binary

Hi HN,

I built TokenWise — a transparent proxy that sits between your app and AI APIs, analyzes each prompt, and routes it to the cheapest capable model.

**The problem:** Most apps send everything to GPT-4 or Claude Opus, burning $0.01–0.03 per call even for simple tasks like "summarize this" or "translate hello."

**The solution:** TokenWise classifies prompts by complexity, then routes:
- Simple ("what is X?") → deepseek-chat ($0.0003/1K tokens)
- Mid-complexity ("explain how X works") → qwen-max ($0.0008/1K)
- Complex ("debug this Rust code step by step") → deepseek-reasoner ($0.001/1K)

**How it works:**
1. Run `tokenwise start` (or `docker run`)
2. Point your app's `OPENAI_BASE_URL` to `http://localhost:9401/v1`
3. TokenWise intercepts every `/chat/completions` request, rewrites the model, and forwards to the right upstream

Your app thinks it's talking to one model. TokenWise is picking the cheapest one that can do the job.

**By the numbers (from my testing):**
- "What's the capital of France?" → deepseek-chat: $0.000007 (vs $0.0001+ on GPT-4)
- "Debug this Rust async deadlock..." → deepseek-reasoner auto-routed, $0.003 for a full analysis
- Overall savings: 70-90% on API bills

**Tech stack:** Rust, async (tokio + hyper), SQLite (local storage), HTMX dashboard, zero external services.

**License:** Free tier (3 providers). Pro key for unlimited providers + safety net fallback.

**Try it:**
- GitHub: https://github.com/chentang1127-hub/tokenwise
- `brew install chentang1127-hub/tap/tokenwise`
- `docker run -p 9400:9400 -p 9401:9401 -e DEEPSEEK_API_KEY=sk-... ghcr.io/chentang1127-hub/tokenwise:latest`

Happy to answer questions and take feedback. What would make this more useful for your stack?
