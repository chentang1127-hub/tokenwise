# Reddit Posts

## r/rust — Showcase + Technical Deep-Dive

Title: `[Media] TokenWise — a Rust proxy that cut my AI API costs by 83%`

Body:

I built this over 2 weeks to solve a real problem: my AI-powered apps were burning money sending every request to expensive models.

**Stack:**
- `hyper` 1.x with custom `Service<Incoming>` trait impl
- `tokio` for async runtime
- `rusqlite` for local call recording (WAL mode)
- `askama` for compile-time HTML templates (EN + ZH)
- `hmac` + `sha2` for Pro license verification
- `clap` for CLI with subcommands

**Technical highlights:**
- **TeeStream:** SSE streaming is tricky — you need to forward chunks to the client AND capture token usage. Solution: unbounded mpsc channel + JoinHandle that resolves when TeeStream drops (client consumed all chunks) and analyzer drained all frames. No more race conditions.
- **Body chain from hell:** `TeeStream → StreamBody → BodyExt::boxed → BoxBody::new(map_err)` — took a while to get the type gymnastics right with hyper 1.x
- **Model rewrite:** Client sends `"model": "gpt-4o-mini"`, proxy routes to `deepseek-chat`, rewrites the JSON body before forwarding. Client never knows.
- **SQLite without ORM:** Raw SQL with typed row mapping. 18 tests, 0.01s to run.

**What I learned:**
- Rust edition 2024 requires turbofish `::<>` syntax for turbofish in more places (Hmac::<Sha256>)
- `hyper::body::Incoming` + `BodyExt::collect()` is efficient for non-streaming but you can't reuse parts
- `cargo clippy -D warnings` catches a LOT of dead code in libraries with external consumers

**Source:** https://github.com/chentang1127-hub/tokenwise | **Release:** https://github.com/chentang1127-hub/tokenwise/releases

---

## r/programming — Product + Value Prop

Title: `Show /r/programming: TokenWise — automatic AI model routing saves 70-90% on API calls`

Body:

You're probably overpaying for AI APIs. Here's why:

Most apps hardcode one model per endpoint. "Summarize this PDF" goes to GPT-4. "What time is it in Tokyo?" also goes to GPT-4. One costs $0.0001, the other $0.01 — but both go to the same model.

TokenWise is an open-source proxy (Rust, MIT license) that classifies each prompt by complexity and picks the cheapest capable model automatically.

**Real numbers from testing:**
| Prompt | Complexity | Routed to | Cost |
|---|---|---|---|
| "Capital of France?" | Simple | deepseek-chat | $0.000007 |
| "Hello in French" | Simple | deepseek-chat | $0.000005 |
| "Debug this Rust async code" | Complex | deepseek-reasoner | $0.003 |

83% average savings in testing.

**How to try it (30 seconds):**
```bash
brew install chentang1127-hub/tap/tokenwise
export DEEPSEEK_API_KEY=sk-...
tokenwise start
# Set OPENAI_BASE_URL=http://127.0.0.1:9401/v1
```

GitHub: https://github.com/chentang1127-hub/tokenwise

Supports OpenAI, Anthropic, DeepSeek, + 7 Chinese AI providers. All data stays local (SQLite). Pro license for power users.
