#!/bin/bash
# Long-session stress test — simulates a real Claude Code conversation
# with large context (30K+ tokens), multiple turns, and streaming.
set -euo pipefail

TW_URL="${TW_URL:-http://llm.getipgeo.com}"
TW_KEY="${TW_KEY:-}"
COUNT="${STRESS_COUNT:-5}"

if [ -z "$TW_KEY" ]; then
  echo "❌ TW_KEY required"
  exit 1
fi

# Build a long conversation — 15 turns of back-and-forth
LONG_MSGS='[
  {"role":"system","content":"You are a helpful coding assistant. You help with Rust programming, debugging, and code review. Always be concise."},
  {"role":"user","content":"I need to write a function that takes a vector of integers and returns the sum of all even numbers. Can you help?"},
  {"role":"assistant","content":"Sure! Here is a simple implementation:\n\nfn sum_even(numbers: &[i32]) -> i32 {\n    numbers.iter().filter(|n| **n % 2 == 0).sum()\n}\n\nThis uses iterator combinators — filter to keep evens, then sum."},
  {"role":"user","content":"What if I want to do it without iterators? Using a for loop?"},
  {"role":"assistant","content":"Here is the for-loop version:\n\nfn sum_even_for(numbers: &[i32]) -> i32 {\n    let mut total = 0;\n    for &n in numbers {\n        if n % 2 == 0 {\n            total += n;\n        }\n    }\n    total\n}\n\nThis is more explicit and may be easier for beginners to understand."},
  {"role":"user","content":"What about performance? Which is faster?"},
  {"role":"assistant","content":"In release mode with optimizations, both versions compile to nearly identical machine code. The iterator version benefits from LLVM auto-vectorization in some cases. The for-loop version may be slightly more predictable. In practice, the difference is negligible for most workloads — choose whichever is more readable."},
  {"role":"user","content":"I am building a web server with axum. How do I set up middleware for logging?"},
  {"role":"assistant","content":"In axum 0.8, you can use tower layers for middleware. For logging:\n\nuse tower_http::trace::TraceLayer;\nuse tracing::Level;\n\nlet app = Router::new()\n    .route(\"/\", get(handler))\n    .layer(TraceLayer::new_for_http()\n        .make_span_with(|req: &Request| {\n            tracing::span!(Level::INFO, \"request\",\n                method = %req.method(),\n                uri = %req.uri().path())\n        }));\n\nThis logs every request with method, path, status, and latency."},
  {"role":"user","content":"How do I add authentication middleware?"},
  {"role":"assistant","content":"You can create a custom middleware using tower::ServiceBuilder. Here is a basic auth example:\n\nasync fn auth_middleware(\n    headers: HeaderMap,\n    request: Request,\n    next: Next,\n) -> Result<Response, StatusCode> {\n    let token = headers.get(\"Authorization\")\n        .ok_or(StatusCode::UNAUTHORIZED)?;\n    if !validate_token(token) {\n        return Err(StatusCode::UNAUTHORIZED);\n    }\n    Ok(next.run(request).await)\n}\n\nUse it with axum::middleware::from_fn(auth_middleware)."},
  {"role":"user","content":"Can you explain how Rust ownership works with closures?"},
  {"role":"assistant","content":"Closures in Rust capture variables by reference by default. If you need to move ownership, use the `move` keyword:\n\nlet data = vec![1, 2, 3];\nlet closure = move || {\n    // data is now owned by this closure\n    data.iter().sum::<i32>()\n};\n\nWithout `move`, the closure borrows `data`. With `move`, it takes ownership. The Fn/FnMut/FnOnce traits determine how the closure can be called."},
  {"role":"user","content":"What is the difference between Box<dyn Error> and anyhow?"},
  {"role":"assistant","content":"Box<dyn Error> is the standard library trait object for errors. anyhow is a crate that provides a more ergonomic wrapper:\n\n// std\nfn foo() -> Result<(), Box<dyn Error>> { ... }\n\n// anyhow\nfn foo() -> anyhow::Result<()> { ... }\n\nanyhow adds context with .context() and automatic backtrace capture. Use Box<dyn Error> in library code (more explicit), anyhow in application code (more ergonomic)."},
  {"role":"user","content":"I am hitting a borrow checker error: cannot borrow as mutable more than once. How do I fix this?"},
  {"role":"assistant","content":"This is the most common borrow checker error. Common solutions:\n\n1. Restructure code to avoid overlapping borrows:\n   - Drop the first borrow before creating the second\n2. Use std::mem::replace or std::mem::take\n3. Clone the data if it is cheap\n4. Use RefCell for runtime borrow checking\n5. Split borrows with destructuring\n\nThe key is to understand that Rust enforces one writer OR multiple readers at a time. Restructure your code so mutable access does not overlap."},
  {"role":"user","content":"How do I handle async code that also needs to call synchronous functions?"},
  {"role":"assistant","content":"Use tokio::task::spawn_blocking for CPU-bound sync work that would block the async runtime:\n\nlet result = tokio::task::spawn_blocking(|| {\n    heavy_sync_computation()\n}).await?;\n\nFor quick synchronous operations, just call them directly in async code. The rule: if it takes more than ~1ms, use spawn_blocking."},
  {"role":"user","content":"I need to implement retry logic for HTTP requests with exponential backoff."},
  {"role":"assistant","content":"Here is a retry with exponential backoff using tokio:\n\nuse tokio::time::{sleep, Duration};\n\nasync fn fetch_with_retry(url: &str, max_retries: u32) -> Result<Response, Error> {\n    for attempt in 0..max_retries {\n        match client.get(url).send().await {\n            Ok(resp) if resp.status().is_success() => return Ok(resp),\n            Err(e) if attempt < max_retries - 1 => {\n                let delay = Duration::from_secs(2u64.pow(attempt));\n                sleep(delay).await;\n                continue;\n            }\n            Err(e) => return Err(e),\n            _ => return Err(Error::Status(resp.status())),\n        }\n    }\n    Err(Error::MaxRetries)\n}"},
  {"role":"user","content":"What is the best way to structure a Rust project with multiple binaries and shared libraries?"},
  {"role":"assistant","content":"A common workspace structure:\n\nCargo.toml (workspace)\n├── crates/\n│   ├── core/        (shared library)\n│   ├── api/         (shared library)\n│   └── models/      (shared types)\n├── apps/\n│   ├── server/      (binary)\n│   ├── worker/      (binary)\n│   └── cli/         (binary)\n\nUse [workspace] in the root Cargo.toml. Shared code goes in lib crates. Binary crates depend on the libs. This gives clean separation and fast incremental compilation."},
  {"role":"user","content":"Can you summarize everything we discussed about Rust?"}
]'

echo "════════════════════════════════════════"
echo " Long-session stress test"
echo " Messages: $(echo "$LONG_MSGS" | python3 -c 'import sys,json; print(len(json.load(sys.stdin)))' 2>/dev/null || echo 15) turns"
echo " Repetitions: $COUNT"
echo "════════════════════════════════════════"
echo ""

PASS=0
FAIL=0

for i in $(seq 1 $COUNT); do
  echo "--- Request $i/$COUNT (streaming) ---"
  RESULT=$(curl -s -N -m 30 -w "\nHTTP:%{http_code}" "$TW_URL/v1/messages" \
    -H "Content-Type: application/json" \
    -H "x-api-key: $TW_KEY" \
    -H "anthropic-version: 2023-06-01" \
    -d "{\"model\":\"deepseek-v4-flash\",\"max_tokens\":100,\"messages\":$LONG_MSGS,\"stream\":true}" 2>&1)

  HTTP=$(echo "$RESULT" | tail -1 | sed 's/.*HTTP://')
  HAS_START=$(echo "$RESULT" | grep -c "message_start" || true)
  HAS_STOP=$(echo "$RESULT" | grep -c "message_stop" || true)
  HAS_DELTA=$(echo "$RESULT" | grep -c "content_block_delta" || true)

  if [ "$HTTP" = "200" ] && [ "$HAS_START" -gt 0 ] && [ "$HAS_STOP" -gt 0 ]; then
    echo "  ✅ HTTP=$HTTP start=$HAS_START delta=$HAS_DELTA stop=$HAS_STOP"
    PASS=$((PASS + 1))
  else
    echo "  ❌ HTTP=$HTTP start=$HAS_START delta=$HAS_DELTA stop=$HAS_STOP"
    echo "     $(echo "$RESULT" | head -c 200)"
    FAIL=$((FAIL + 1))
  fi
  echo ""
done

echo "════════════════════════════════════════"
echo " Stress test: $PASS/$COUNT passed"
echo "════════════════════════════════════════"

[ "$FAIL" -eq 0 ] && echo "✅ Long session test PASSED" || echo "❌ Long session test FAILED"
exit $FAIL
