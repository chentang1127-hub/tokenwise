//! Inline "Try It" chat widget — rendered as a raw HTML string to avoid
//! Askama's compile-time template size limits (the CSS+JS block overflows
//! the proc-macro stack on Windows GNU toolchain).
//!
//! `{PROXY_URL}` is replaced with the actual proxy base URL at render time.
//! `{DEMO_URL}` is replaced with the admin demo endpoint URL.
//! The user's API key stays in the browser — JavaScript calls the proxy
//! (port 9401) directly, never touching the admin server (port 9400).
//!
//! Features:
//!   - Stream toggle with typewriter effect + live token counter
//!   - Multi-key management (label + provider, stored in localStorage only)
//!   - Auto-migration from legacy single-key format

pub fn render(proxy_url: &str, demo_url: &str) -> String {
    WIDGET_HTML
        .replace("{PROXY_URL}", proxy_url)
        .replace("{DEMO_URL}", demo_url)
}

const WIDGET_HTML: &str = r#"<!-- ─── Try It Chat Widget (collapsible) ─────────────── -->
<details class="try-it-collapse">
<summary class="try-it-summary">&#128172; Try It &mdash; test directly in your browser</summary>
<div class="try-it-section" id="try-it">
  <h3 class="try-it-title">&#128172; Try It</h3>
  <p class="try-it-sub" id="try-it-sub-text">Paste your API key and send a message. Your key stays in your browser &mdash; we never see it.</p>

  <div class="try-it-card">
    <!-- Key Management -->
    <div class="try-it-field" id="try-it-key-field">
      <label for="try-it-key">Your API Key <span id="try-it-key-saved" style="display:none;color:var(--green);font-weight:400;text-transform:none;letter-spacing:0">&#10003; saved</span></label>
      <div class="try-it-key-row">
        <input type="password" id="try-it-key" placeholder="sk-..." autocomplete="off" spellcheck="false" style="flex:1;" />
        <button id="try-it-save-key" class="try-it-mini-btn" onclick="tryItSaveKey()" title="Save this key with a label">+ Save</button>
      </div>
      <div class="try-it-key-meta" id="try-it-key-meta" style="display:none;">
        <input type="text" id="try-it-key-label" placeholder="Label (e.g. DeepSeek Work)" maxlength="40" />
        <select id="try-it-key-provider">
          <option value="">Auto-detect</option>
          <option value="openai">OpenAI</option>
          <option value="anthropic">Anthropic</option>
          <option value="deepseek">DeepSeek</option>
          <option value="google">Google</option>
          <option value="groq">Groq</option>
          <option value="mistral">Mistral</option>
          <option value="xai">xAI</option>
          <option value="openrouter">OpenRouter</option>
        </select>
        <button class="try-it-mini-btn try-it-save-confirm" onclick="tryItConfirmSave()">Save</button>
      </div>
    </div>
    <div class="try-it-field" id="try-it-saved-keys" style="display:none;">
      <label>Saved Keys</label>
      <div id="try-it-keys-list" class="try-it-keys-list"></div>
    </div>

    <!-- Message input -->
    <div class="try-it-field">
      <label for="try-it-msg">Message</label>
      <textarea id="try-it-msg" rows="2" placeholder="Say something to test the proxy&hellip;"></textarea>
    </div>

    <!-- Stream toggle + buttons -->
    <div class="try-it-field try-it-stream-row">
      <label class="try-it-checkbox-label">
        <input type="checkbox" id="try-it-stream" />
        <span>Stream response (typewriter effect)</span>
      </label>
      <span id="try-it-token-counter" class="try-it-token-counter" style="display:none">0 tokens</span>
    </div>

    <div class="try-it-buttons">
      <button id="try-it-send" class="try-it-send-btn" onclick="tryItSend()">Send with Key &#10132;</button>
      <button id="try-it-demo" class="try-it-demo-btn" onclick="tryItDemo()">Try Demo &#9678;</button>
    </div>

    <div id="try-it-loading" class="try-it-loading" style="display:none;">
      <span class="try-it-spinner"></span> <span id="try-it-loading-text">Waiting for response&hellip;</span>
    </div>
    <div id="try-it-error" class="try-it-error" style="display:none;"></div>
    <div id="try-it-result" class="try-it-result" style="display:none;"></div>
  </div>
</div>

<style>
.try-it-collapse { margin-top: 32px; }
.try-it-collapse > .try-it-section { margin-top: 16px; }
.try-it-summary { font-size: 14px; font-weight: 600; color: var(--muted); cursor: pointer; padding: 8px 0; user-select: none; list-style: none; }
.try-it-summary::-webkit-details-marker { display: none; }
.try-it-summary::before { content: '\25B6'; display: inline-block; margin-right: 8px; font-size: 10px; transition: transform 0.15s; }
.try-it-collapse[open] > .try-it-summary::before { transform: rotate(90deg); }
.try-it-summary:hover { color: var(--text); }
.try-it-section { }
.try-it-title { font-size: 16px; font-weight: 600; margin-bottom: 4px; }
.try-it-sub { font-size: 12px; color: var(--muted); margin-bottom: 14px; max-width: 540px; }
.try-it-card { background: rgba(255,255,255,0.02); border: 1px solid var(--border); border-radius: 10px; padding: 20px; max-width: 600px; }
.try-it-field { margin-bottom: 12px; }
.try-it-field label { display: block; font-size: 11px; color: var(--muted); margin-bottom: 4px; text-transform: uppercase; letter-spacing: 0.06em; font-weight: 600; }
.try-it-field input, .try-it-field textarea { width: 100%; padding: 10px 12px; background: var(--bg); border: 1px solid var(--border); border-radius: 6px; color: var(--text); font-size: 13px; font-family: inherit; }
.try-it-field textarea { resize: vertical; min-height: 48px; }
.try-it-field input:focus, .try-it-field textarea:focus { outline: none; border-color: var(--blue); }
.try-it-field input::placeholder, .try-it-field textarea::placeholder { color: var(--muted); opacity: 0.45; }

/* ── Key Management ─── */
.try-it-key-row { display: flex; gap: 8px; align-items: center; }
.try-it-mini-btn { padding: 6px 14px; border-radius: 6px; font-size: 12px; font-weight: 500; cursor: pointer; border: 1px solid var(--border); background: transparent; color: var(--muted); white-space: nowrap; transition: all 0.15s; }
.try-it-mini-btn:hover { color: var(--text); border-color: var(--muted); }
.try-it-save-confirm { color: var(--green); border-color: var(--green); }
.try-it-save-confirm:hover { background: var(--green); color: #000; }
.try-it-key-meta { display: flex; gap: 8px; margin-top: 8px; align-items: center; }
.try-it-key-meta input { flex: 2; padding: 6px 10px; background: var(--bg); border: 1px solid var(--border); border-radius: 4px; color: var(--text); font-size: 12px; }
.try-it-key-meta select { flex: 1; padding: 6px 10px; background: var(--bg); border: 1px solid var(--border); border-radius: 4px; color: var(--text); font-size: 12px; }
.try-it-keys-list { display: flex; flex-direction: column; gap: 4px; }
.try-it-key-item { display: flex; align-items: center; gap: 10px; padding: 8px 10px; background: rgba(255,255,255,0.03); border: 1px solid var(--border); border-radius: 6px; font-size: 12px; }
.try-it-key-item.active { border-color: var(--green); background: rgba(34,197,94,0.06); }
.try-it-key-item .key-label { flex: 1; color: var(--text); font-weight: 500; }
.try-it-key-item .key-provider { color: var(--muted); font-size: 10px; text-transform: uppercase; }
.try-it-key-item .key-select { padding: 3px 10px; border-radius: 4px; font-size: 11px; cursor: pointer; border: 1px solid var(--border); background: transparent; color: var(--blue); white-space: nowrap; }
.try-it-key-item .key-select:hover { background: var(--blue); color: #fff; border-color: var(--blue); }
.try-it-key-item .key-delete { padding: 3px 8px; border-radius: 4px; font-size: 11px; cursor: pointer; border: 1px solid transparent; background: transparent; color: var(--red); }
.try-it-key-item .key-delete:hover { background: rgba(239,68,68,0.1); border-color: var(--red); }

/* ── Stream Toggle ─── */
.try-it-stream-row { display: flex; align-items: center; justify-content: space-between; }
.try-it-checkbox-label { display: flex; align-items: center; gap: 8px; font-size: 12px; color: var(--muted); cursor: pointer; user-select: none; text-transform: none; letter-spacing: 0; font-weight: 400; }
.try-it-checkbox-label input[type="checkbox"] { width: auto; accent-color: var(--green); width: 16px; height: 16px; cursor: pointer; }
.try-it-token-counter { font-size: 11px; color: var(--blue); font-family: 'SF Mono', 'Cascadia Code', 'Consolas', monospace; font-weight: 600; }
.try-it-result.streaming { border-left: 2px solid var(--blue); }
.try-it-cursor { display: inline-block; width: 8px; height: 14px; background: var(--blue); margin-left: 2px; animation: try-it-blink 0.7s step-end infinite; vertical-align: text-bottom; }
@keyframes try-it-blink { 50% { opacity: 0; } }

/* ── Buttons & States ─── */
.try-it-buttons { display: flex; gap: 10px; flex-wrap: wrap; }
.try-it-send-btn { padding: 9px 22px; border-radius: 8px; font-weight: 600; font-size: 13px; cursor: pointer; border: 1px solid var(--green); background: transparent; color: var(--green); transition: all 0.15s; }
.try-it-send-btn:hover { background: var(--green); color: #000; }
.try-it-send-btn:disabled { opacity: 0.45; cursor: not-allowed; }
.try-it-demo-btn { padding: 9px 22px; border-radius: 8px; font-weight: 600; font-size: 13px; cursor: pointer; border: 1px solid var(--purple); background: transparent; color: var(--purple); transition: all 0.15s; }
.try-it-demo-btn:hover { background: var(--purple); color: #fff; }
.try-it-demo-btn:disabled { opacity: 0.45; cursor: not-allowed; }
.try-it-loading { margin-top: 14px; font-size: 13px; color: var(--muted); display: flex; align-items: center; gap: 8px; }
.try-it-spinner { display: inline-block; width: 14px; height: 14px; border: 2px solid var(--border); border-top-color: var(--blue); border-radius: 50%; animation: try-it-spin 0.6s linear infinite; }
@keyframes try-it-spin { to { transform: rotate(360deg); } }
.try-it-error { margin-top: 14px; padding: 12px 14px; background: rgba(239,68,68,0.08); border: 1px solid rgba(239,68,68,0.25); border-radius: 8px; font-size: 13px; color: var(--red); line-height: 1.5; word-break: break-word; }
.try-it-result { margin-top: 14px; padding: 14px; background: var(--bg); border: 1px solid var(--border); border-radius: 8px; font-size: 13px; line-height: 1.65; white-space: pre-wrap; word-break: break-word; }
.try-it-usage { margin-top: 10px; padding-top: 10px; border-top: 1px solid var(--border); font-size: 11px; color: var(--muted); display: flex; gap: 18px; flex-wrap: wrap; }
.try-it-demo-badge { display: inline-block; font-size: 10px; background: rgba(168,85,247,0.15); color: var(--purple); padding: 2px 8px; border-radius: 4px; margin-bottom: 8px; text-transform: uppercase; letter-spacing: 0.05em; }
</style>

<script>
// ── HTML escape ──────────────────────────────────────
function escapeHtml(text) {
  var div = document.createElement('div');
  div.appendChild(document.createTextNode(text));
  return div.innerHTML;
}

// ── Multi-Key Management ────────────────────────────
function tryItLoadKeys() {
  try {
    var raw = localStorage.getItem('tokenwise_keys');
    return raw ? JSON.parse(raw) : [];
  } catch (_) { return []; }
}

function tryItSaveKeys(keys) {
  try { localStorage.setItem('tokenwise_keys', JSON.stringify(keys)); } catch (_) {}
}

function tryItRenderKeys() {
  var keys = tryItLoadKeys();
  var activeIdx = parseInt(localStorage.getItem('tokenwise_active_key_index') || '-1');
  var container = document.getElementById('try-it-keys-list');
  var section = document.getElementById('try-it-saved-keys');

  if (keys.length === 0) { section.style.display = 'none'; return; }
  section.style.display = 'block';

  var html = '';
  for (var i = 0; i < keys.length; i++) {
    var k = keys[i];
    var isActive = i === activeIdx;
    html += '<div class="try-it-key-item' + (isActive ? ' active' : '') + '">'
          + '<span class="key-label">' + escapeHtml(k.label) + '</span>'
          + (k.provider ? '<span class="key-provider">' + escapeHtml(k.provider) + '</span>' : '')
          + '<button class="key-select" onclick="tryItSelectKey(' + i + ')">' + (isActive ? 'Active' : 'Use') + '</button>'
          + '<button class="key-delete" onclick="tryItDeleteKey(' + i + ')">&times;</button>'
          + '</div>';
  }
  container.innerHTML = html;

  // Restore active key into input
  if (activeIdx >= 0 && keys[activeIdx]) {
    document.getElementById('try-it-key').value = keys[activeIdx].key;
    document.getElementById('try-it-key-saved').style.display = 'inline';
    document.getElementById('try-it-key-saved').textContent = '✓ ' + keys[activeIdx].label;
  }
}

// ── Save key flow ───────────────────────────────────
function tryItSaveKey() {
  var keyInput = document.getElementById('try-it-key');
  if (!keyInput.value.trim()) { return; }
  document.getElementById('try-it-key-meta').style.display = 'flex';
  document.getElementById('try-it-key-label').focus();
}

function tryItConfirmSave() {
  var key = document.getElementById('try-it-key').value.trim();
  var label = document.getElementById('try-it-key-label').value.trim() || 'Key ' + (tryItLoadKeys().length + 1);
  var provider = document.getElementById('try-it-key-provider').value;

  var keys = tryItLoadKeys();
  keys.push({ label: label, key: key, provider: provider });
  tryItSaveKeys(keys);

  document.getElementById('try-it-key-meta').style.display = 'none';
  document.getElementById('try-it-key-label').value = '';
  tryItRenderKeys();
}

function tryItSelectKey(idx) {
  localStorage.setItem('tokenwise_active_key_index', idx.toString());
  tryItRenderKeys();
}

function tryItDeleteKey(idx) {
  var keys = tryItLoadKeys();
  keys.splice(idx, 1);
  tryItSaveKeys(keys);

  var activeIdx = parseInt(localStorage.getItem('tokenwise_active_key_index') || '-1');
  if (activeIdx === idx) {
    localStorage.setItem('tokenwise_active_key_index', '-1');
    document.getElementById('try-it-key').value = '';
    document.getElementById('try-it-key-saved').style.display = 'none';
  } else if (activeIdx > idx) {
    localStorage.setItem('tokenwise_active_key_index', (activeIdx - 1).toString());
  }
  tryItRenderKeys();
}

// ── Init: migrate legacy key + render saved keys ────
(function() {
  try {
    var oldKey = localStorage.getItem('tokenwise_api_key');
    if (oldKey && !localStorage.getItem('tokenwise_keys')) {
      tryItSaveKeys([{ label: 'Default Key', key: oldKey, provider: '' }]);
      localStorage.setItem('tokenwise_active_key_index', '0');
      localStorage.removeItem('tokenwise_api_key');
    }
    tryItRenderKeys();
  } catch (_) {}
})();

// ── Shared: show result ─────────────────────────────
function tryItShowResult(content, usage, isDemo, isStreaming) {
  var resEl = document.getElementById('try-it-result');
  var html = '';
  if (isDemo) {
    html += '<div class="try-it-demo-badge">Demo &mdash; no API key used</div>';
  }
  html += isStreaming ? content : escapeHtml(content);
  if (usage) {
    html += '<div class="try-it-usage">'
          + '<span>&#9654; prompt: ' + (usage.prompt_tokens||0) + '</span>'
          + '<span>&#9632; completion: ' + (usage.completion_tokens||0) + '</span>'
          + '<span>&#9312; total: ' + (usage.total_tokens||0) + '</span>';
    if (usage.cost_usd != null) {
      html += '<span>&#36;' + usage.cost_usd.toFixed(6) + '</span>';
    }
    html += '</div>';
  }
  resEl.innerHTML = html;
  resEl.style.display = 'block';
}

// ── Clear previous output ───────────────────────────
function tryItReset() {
  document.getElementById('try-it-result').style.display = 'none';
  document.getElementById('try-it-result').innerHTML = '';
  document.getElementById('try-it-result').classList.remove('streaming');
  document.getElementById('try-it-error').style.display = 'none';
  document.getElementById('try-it-error').textContent = '';
  document.getElementById('try-it-loading').style.display = 'none';
  document.getElementById('try-it-token-counter').style.display = 'none';
  document.getElementById('try-it-token-counter').textContent = '0 tokens';
}

// ── Real API call (needs key, supports streaming) ───
async function tryItSend() {
  var keyInput = document.getElementById('try-it-key');
  var key = keyInput.value.trim();
  var msg = document.getElementById('try-it-msg').value.trim();
  var btn = document.getElementById('try-it-send');
  var loadEl = document.getElementById('try-it-loading');
  var loadText = document.getElementById('try-it-loading-text');
  var errEl = document.getElementById('try-it-error');
  var useStream = document.getElementById('try-it-stream').checked;

  tryItReset();

  // Auto-fallback to active saved key
  if (!key) {
    var keys = tryItLoadKeys();
    var activeIdx = parseInt(localStorage.getItem('tokenwise_active_key_index') || '-1');
    if (activeIdx >= 0 && keys[activeIdx]) {
      key = keys[activeIdx].key;
      keyInput.value = key;
    } else {
      errEl.textContent = 'Please enter your API key first, or click "Try Demo" to test without a key.';
      errEl.style.display = 'block';
      keyInput.focus();
      return;
    }
  }
  if (!msg) {
    errEl.textContent = 'Please enter a message.';
    errEl.style.display = 'block';
    document.getElementById('try-it-msg').focus();
    return;
  }

  btn.disabled = true;
  document.getElementById('try-it-demo').disabled = true;
  loadText.textContent = useStream ? 'Streaming from AI provider&hellip;' : 'Sending to AI provider&hellip;';
  loadEl.style.display = 'flex';

  var auth = key.indexOf('Bearer ') === 0 ? key : 'Bearer ' + key;

  try {
    var resp = await fetch('{PROXY_URL}/chat/completions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'Authorization': auth },
      body: JSON.stringify({
        model: 'deepseek-chat',
        messages: [{ role: 'user', content: msg }],
        stream: useStream,
      }),
    });

    if (!resp.ok) {
      loadEl.style.display = 'none';
      btn.disabled = false;
      document.getElementById('try-it-demo').disabled = false;
      var detail = '';
      try { var errJson = await resp.json(); detail = errJson.error || JSON.stringify(errJson); } catch (_) {}
      errEl.textContent = 'HTTP ' + resp.status + (detail ? ': ' + detail : '');
      errEl.style.display = 'block';
      return;
    }

    if (useStream) {
      // ── Streaming path: SSE parsing with typewriter effect ──
      var reader = resp.body.getReader();
      var decoder = new TextDecoder();
      var buffer = '';
      var fullContent = '';
      var totalTokens = 0;
      var usage = null;
      var resEl = document.getElementById('try-it-result');
      var tokenCounter = document.getElementById('try-it-token-counter');

      loadEl.style.display = 'none';
      btn.disabled = false;
      document.getElementById('try-it-demo').disabled = false;

      resEl.innerHTML = '<span class="try-it-cursor"></span>';
      resEl.style.display = 'block';
      resEl.classList.add('streaming');
      tokenCounter.style.display = 'inline';

      while (true) {
        var readResult = await reader.read();
        if (readResult.done) break;
        buffer += decoder.decode(readResult.value, { stream: true });
        var lines = buffer.split('\n');
        buffer = lines.pop() || '';
        for (var i = 0; i < lines.length; i++) {
          var line = lines[i].trim();
          if (!line.startsWith('data: ')) continue;
          var data = line.slice(6);
          if (data === '[DONE]') continue;
          try {
            var chunk = JSON.parse(data);
            var delta = chunk.choices && chunk.choices[0] && chunk.choices[0].delta;
            if (delta && delta.content) {
              fullContent += delta.content;
              resEl.innerHTML = escapeHtml(fullContent) + '<span class="try-it-cursor"></span>';
            }
            if (chunk.usage) {
              usage = chunk.usage;
              totalTokens = usage.total_tokens || 0;
              tokenCounter.textContent = totalTokens + ' tokens';
            }
          } catch (_) { /* skip parse errors */ }
        }
      }

      resEl.classList.remove('streaming');
      resEl.innerHTML = escapeHtml(fullContent);
      if (usage) {
        usage.cost_usd = usage.cost_usd || 0;
        tryItShowResult(fullContent, usage, false, true);
      } else {
        tokenCounter.textContent = totalTokens ? totalTokens + ' tokens' : '';
        tryItShowResult(fullContent, { total_tokens: totalTokens }, false, true);
      }

      // Save key on success
      try { localStorage.setItem('tokenwise_api_key', key); } catch (_) {}
      document.getElementById('try-it-key-saved').style.display = 'inline';

    } else {
      // ── Non-streaming path (original behaviour) ──
      loadEl.style.display = 'none';
      btn.disabled = false;
      document.getElementById('try-it-demo').disabled = false;

      var data = await resp.json();
      var content = (data.choices&&data.choices[0]&&data.choices[0].message&&data.choices[0].message.content)
                 || '(No content in response)';
      tryItShowResult(content, data.usage, false, false);

      // Save key on success
      try { localStorage.setItem('tokenwise_api_key', key); } catch (_) {}
      document.getElementById('try-it-key-saved').style.display = 'inline';
    }

  } catch (e) {
    loadEl.style.display = 'none';
    btn.disabled = false;
    document.getElementById('try-it-demo').disabled = false;
    errEl.textContent = 'Connection failed: ' + (e.message||e) + '. Make sure TokenWise Core is running.';
    errEl.style.display = 'block';
  }
}

// ── Demo call (no key needed) ──
async function tryItDemo() {
  var msg   = document.getElementById('try-it-msg').value.trim();
  var btn   = document.getElementById('try-it-demo');
  var loadEl = document.getElementById('try-it-loading');
  var loadText = document.getElementById('try-it-loading-text');
  var errEl = document.getElementById('try-it-error');

  tryItReset();

  if (!msg) {
    errEl.textContent = 'Please enter a message.';
    errEl.style.display = 'block';
    document.getElementById('try-it-msg').focus();
    return;
  }

  btn.disabled = true;
  document.getElementById('try-it-send').disabled = true;
  loadText.textContent = 'Running demo (no API key needed)&hellip;';
  loadEl.style.display = 'flex';

  try {
    var resp = await fetch('{DEMO_URL}', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ message: msg }),
    });

    loadEl.style.display = 'none';
    btn.disabled = false;
    document.getElementById('try-it-send').disabled = false;

    if (!resp.ok) {
      errEl.textContent = 'Demo failed: HTTP ' + resp.status;
      errEl.style.display = 'block';
      return;
    }

    var data = await resp.json();
    var content = (data.choices&&data.choices[0]&&data.choices[0].message&&data.choices[0].message.content)
               || '(No content)';
    tryItShowResult(content, data.usage, true, false);
  } catch (e) {
    loadEl.style.display = 'none';
    btn.disabled = false;
    document.getElementById('try-it-send').disabled = false;
    errEl.textContent = 'Demo failed: ' + (e.message||e) + '. Make sure TokenWise Core is running.';
    errEl.style.display = 'block';
  }
}
</script>
</details>
"#;
