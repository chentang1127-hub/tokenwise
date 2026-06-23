"""Long-session stress test — simulate real Claude Code conversations."""
import json, os, sys, time
import urllib.request, urllib.error

TW_URL = os.getenv("TW_URL", "http://llm.getipgeo.com")
TW_KEY = os.getenv("TW_KEY", "")
COUNT = int(os.getenv("STRESS_COUNT", "5"))

if not TW_KEY:
    print("❌ TW_KEY required")
    sys.exit(1)

# Build a long conversation — 15 turns
msgs = [
    {"role": "system", "content": "You are a helpful coding assistant. You help with Rust programming, debugging, and code review. Always be concise."},
    {"role": "user", "content": "Write a function that sums even numbers from a vector. Answer with just the code, no explanation."},
    {"role": "assistant", "content": "fn sum_even(numbers: &[i32]) -> i32 { numbers.iter().filter(|n| **n % 2 == 0).sum() }"},
    {"role": "user", "content": "What is the time complexity of that function? Answer in one sentence."},
    {"role": "assistant", "content": "O(n) — it iterates through the slice once, and each operation (filter, sum) is constant time per element."},
    {"role": "user", "content": "How do I set up axum logging middleware? Give a short code example."},
    {"role": "assistant", "content": "use tower_http::trace::TraceLayer; // Add to router: .layer(TraceLayer::new_for_http()) — this logs method, path, status, and latency for every request."},
    {"role": "user", "content": "How do I handle errors with anyhow vs Box<dyn Error>? Short answer."},
    {"role": "assistant", "content": "Use Box<dyn Error> in library code (explicit types), use anyhow::Result in application code (ergonomic .context() and backtraces)."},
    {"role": "user", "content": "Explain Rust ownership with closures in 2 sentences."},
    {"role": "assistant", "content": "Closures capture variables by reference by default. Use the 'move' keyword to transfer ownership into the closure instead."},
    {"role": "user", "content": "How do I implement retry with exponential backoff in tokio? Give code."},
    {"role": "assistant", "content": "for attempt in 0..max_retries { match req.send().await { Ok(r) => return Ok(r), Err(e) => { sleep(Duration::from_secs(2u64.pow(attempt))).await; } } }"},
    {"role": "user", "content": "What is the best project structure for a Rust workspace with multiple binaries? Answer in 3 lines."},
    {"role": "assistant", "content": "Root Cargo.toml with [workspace] members. Shared code in crates/ as lib crates. Binaries in apps/ depending on shared crates."},
    {"role": "user", "content": "Summarize everything we discussed in one paragraph."},
]

# Pad with a large system prompt to reach 30K+ tokens
padding = "You are an expert Rust systems programmer with deep knowledge of LLM infrastructure, networking protocols, proxy architecture, SQLite, async Rust, and production deployment. " * 200
msgs[0]["content"] = msgs[0]["content"] + " " + padding

total_chars = sum(len(m["content"]) for m in msgs)
est_tokens = total_chars // 4
print(f"Payload: {len(msgs)} messages, ~{est_tokens:,} tokens ({total_chars:,} chars)")
print(f"Repetitions: {COUNT}")
print(f"Target: {TW_URL}")
print()

pass_count = 0
fail_count = 0

for i in range(COUNT):
    print(f"--- Request {i+1}/{COUNT} (streaming) ---", end=" ", flush=True)

    body = json.dumps({
        "model": "deepseek-v4-flash",
        "max_tokens": 100,
        "messages": msgs,
        "stream": True,
    }).encode()

    req = urllib.request.Request(
        f"{TW_URL}/v1/messages",
        data=body,
        headers={
            "Content-Type": "application/json",
            "x-api-key": TW_KEY,
            "anthropic-version": "2023-06-01",
        },
    )

    try:
        t0 = time.time()
        resp = urllib.request.urlopen(req, timeout=30)
        data = resp.read().decode()
        elapsed = time.time() - t0

        has_start = "message_start" in data
        has_stop = "message_stop" in data
        has_delta = "content_block_delta" in data
        blanks = data.count("\n\n")

        if resp.status == 200 and has_start and has_stop:
            print(f"✅ HTTP={resp.status} start={has_start} delta={has_delta} stop={has_stop} blanks={blanks} t={elapsed:.1f}s")
            pass_count += 1
        else:
            print(f"❌ HTTP={resp.status} start={has_start} delta={has_delta} stop={has_stop}")
            print(f"   {data[:200]}")
            fail_count += 1

    except Exception as e:
        print(f"❌ Error: {e}")
        fail_count += 1

print()
print(f"════════════════════════════════════════")
print(f" Stress test: {pass_count}/{COUNT} passed")
print(f"════════════════════════════════════════")

if fail_count == 0:
    print("✅ Long session test PASSED")
else:
    print("❌ Long session test FAILED")

sys.exit(fail_count)
