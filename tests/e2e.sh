#!/bin/bash
# TokenWise e2e test — run before every deployment
# Usage: TW_URL=http://llm.getipgeo.com TW_KEY=sk-xxx ./tests/e2e.sh
set -euo pipefail

TW_URL="${TW_URL:-http://llm.getipgeo.com}"
TW_KEY="${TW_KEY:-}"

if [ -z "$TW_KEY" ]; then
  echo "❌ TW_KEY is required. Set it: TW_KEY=sk-xxx ./tests/e2e.sh"
  exit 1
fi

PASS=0
FAIL=0
red()   { echo -e "\033[31m$*\033[0m"; }
green() { echo -e "\033[32m$*\033[0m"; }

check() {
  local name="$1" expected="$2" actual="$3"
  if echo "$actual" | grep -q "$expected"; then
    green "  ✅ $name"
    PASS=$((PASS + 1))
  else
    red "  ❌ $name (expected: $expected)"
    red "     got: $(echo "$actual" | head -c 200)"
    FAIL=$((FAIL + 1))
  fi
}

echo "════════════════════════════════════════"
echo " TokenWise e2e — $TW_URL"
echo "════════════════════════════════════════"
echo ""

# ── 1. Health ────────────────────────────────────────────
echo "1. Health check"
RESP=$(curl -s "$TW_URL/v1/messages" -H "Content-Type: application/json" \
  -H "x-api-key: $TW_KEY" -H "anthropic-version: 2023-06-01" \
  -d '{"model":"deepseek-v4-flash","max_tokens":1,"messages":[{"role":"user","content":"x"}]}' 2>&1)
# Should NOT contain "502" or "empty" or "malformed"
if echo "$RESP" | grep -qi "502\|empty\|malformed\|error"; then
  red "  ❌ Health: proxy returned error: $(echo "$RESP" | head -c 150)"
  FAIL=$((FAIL + 1))
else
  green "  ✅ Health: proxy responding"
  PASS=$((PASS + 1))
fi

# ── 2. Non-streaming Anthropic ────────────────────────────
echo "2. Non-streaming Anthropic format"
RESP=$(curl -s -w "\n%{http_code}" "$TW_URL/v1/messages" \
  -H "Content-Type: application/json" \
  -H "x-api-key: $TW_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"deepseek-v4-flash","max_tokens":30,"messages":[{"role":"user","content":"reply with exactly: pong"}],"stream":false}' 2>&1)
check "HTTP 200"                 "200" "$(echo "$RESP" | tail -1)"
check "Anthropic content field"  '"text"' "$RESP"
check "Anthropic stop_reason"    '"stop_reason"' "$RESP"

# ── 3. Streaming SSE format ──────────────────────────────
echo "3. Streaming SSE format"
SSE=$(curl -s -N -m 10 "$TW_URL/v1/messages" \
  -H "Content-Type: application/json" \
  -H "x-api-key: $TW_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"deepseek-v4-flash","max_tokens":10,"messages":[{"role":"user","content":"hi"}],"stream":true}' 2>&1)
check "message_start event"   "message_start" "$SSE"
check "content_block_start"   "content_block_start" "$SSE"
check "content_block_delta"   "content_block_delta" "$SSE"
check "message_stop event"    "message_stop" "$SSE"
# Verify \n\n between events: count blank lines > 2
BLANKS=$(echo "$SSE" | grep -c "^$" || true)
if [ "$BLANKS" -ge 2 ]; then
  green "  ✅ SSE blank lines between events ($BLANKS blank lines)"
  PASS=$((PASS + 1))
else
  red "  ❌ SSE blank lines: $BLANKS (need >= 2)"
  FAIL=$((FAIL + 1))
fi

# ── 4. Fake key rejection ────────────────────────────────
echo "4. Fake key rejection"
RESP=$(curl -s "$TW_URL/v1/messages" \
  -H "Content-Type: application/json" \
  -H "x-api-key: deadbeef-fake-key-12345" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"deepseek-v4-flash","max_tokens":5,"messages":[{"role":"user","content":"unique-test-xyz-123"}],"stream":false}' 2>&1)
check "Rejects fake key" "error\|401\|Unauthorized" "$RESP"

# ── 5. OpenAI format ─────────────────────────────────────
echo "5. OpenAI format"
RESP=$(curl -s -w "\n%{http_code}" "$TW_URL/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TW_KEY" \
  -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"reply: pong"}],"max_tokens":5}' 2>&1)
check "HTTP 200"       "200" "$(echo "$RESP" | tail -1)"
check "OpenAI choices" "choices" "$RESP"

# ── Summary ──────────────────────────────────────────────
echo ""
echo "════════════════════════════════════════"
echo " Results: $PASS passed, $FAIL failed"
echo "════════════════════════════════════════"

if [ "$FAIL" -gt 0 ]; then
  red "❌ E2E FAILED — do not deploy"
  exit 1
else
  green "✅ E2E PASSED — ready to ship"
fi
