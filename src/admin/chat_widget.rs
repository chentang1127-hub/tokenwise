//! Inline "Try It" chat widget — rendered as a raw HTML string to avoid
//! Askama's compile-time template size limits (the CSS+JS block overflows
//! the proc-macro stack on Windows GNU toolchain).
//!
//! `{PROXY_URL}` is replaced with the actual proxy base URL at render time.
//! `{DEMO_URL}` is replaced with the admin demo endpoint URL.
//! The user's API key stays in the browser — JavaScript calls the proxy
//! (port 9401) directly, never touching the admin server (port 9400).

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
    <div class="try-it-field" id="try-it-key-field">
      <label for="try-it-key">Your API Key <span id="try-it-key-saved" style="display:none;color:var(--green);font-weight:400;text-transform:none;letter-spacing:0">&#10003; saved</span></label>
      <input type="password" id="try-it-key" placeholder="sk-..." autocomplete="off" spellcheck="false" />
    </div>
    <div class="try-it-field">
      <label for="try-it-msg">Message</label>
      <textarea id="try-it-msg" rows="2" placeholder="Say something to test the proxy&hellip;"></textarea>
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
// ── localStorage: remember the API key across sessions ──
(function() {
  try {
    var saved = localStorage.getItem('tokenwise_api_key');
    if (saved) {
      document.getElementById('try-it-key').value = saved;
      document.getElementById('try-it-key-saved').style.display = 'inline';
    }
  } catch (_) {}
})();

// ── Shared: show the response in the result area ──
function tryItShowResult(content, usage, isDemo) {
  var resEl = document.getElementById('try-it-result');
  var html = '';
  if (isDemo) {
    html += '<div class="try-it-demo-badge">Demo &mdash; no API key used</div>';
  }
  html += content;
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

// ── Clear previous output ──
function tryItReset() {
  document.getElementById('try-it-result').style.display = 'none';
  document.getElementById('try-it-result').innerHTML = '';
  document.getElementById('try-it-error').style.display = 'none';
  document.getElementById('try-it-error').textContent = '';
  document.getElementById('try-it-loading').style.display = 'none';
}

// ── Real API call (needs key) ──
async function tryItSend() {
  var key   = document.getElementById('try-it-key').value.trim();
  var msg   = document.getElementById('try-it-msg').value.trim();
  var btn   = document.getElementById('try-it-send');
  var loadEl = document.getElementById('try-it-loading');
  var loadText = document.getElementById('try-it-loading-text');
  var errEl = document.getElementById('try-it-error');

  tryItReset();

  if (!key) {
    errEl.textContent = 'Please enter your API key first, or click "Try Demo" to test without a key.';
    errEl.style.display = 'block';
    document.getElementById('try-it-key').focus();
    return;
  }
  if (!msg) {
    errEl.textContent = 'Please enter a message.';
    errEl.style.display = 'block';
    document.getElementById('try-it-msg').focus();
    return;
  }

  btn.disabled = true;
  document.getElementById('try-it-demo').disabled = true;
  loadText.textContent = 'Sending to AI provider&hellip;';
  loadEl.style.display = 'flex';

  var auth = key.indexOf('Bearer ') === 0 ? key : 'Bearer ' + key;

  try {
    var resp = await fetch('{PROXY_URL}/chat/completions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'Authorization': auth },
      body: JSON.stringify({
        model: 'deepseek-chat',
        messages: [{ role: 'user', content: msg }],
        stream: false,
      }),
    });

    loadEl.style.display = 'none';
    btn.disabled = false;
    document.getElementById('try-it-demo').disabled = false;

    if (!resp.ok) {
      var detail = '';
      try { var errJson = await resp.json(); detail = errJson.error || JSON.stringify(errJson); } catch (_) {}
      errEl.textContent = 'HTTP ' + resp.status + (detail ? ': ' + detail : '');
      errEl.style.display = 'block';
      return;
    }

    // Save key to localStorage on success
    try { localStorage.setItem('tokenwise_api_key', key); } catch (_) {}
    document.getElementById('try-it-key-saved').style.display = 'inline';

    var data = await resp.json();
    var content = (data.choices&&data.choices[0]&&data.choices[0].message&&data.choices[0].message.content)
               || '(No content in response)';
    tryItShowResult(content, data.usage, false);
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
    tryItShowResult(content, data.usage, true);
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
