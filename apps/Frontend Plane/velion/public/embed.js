/* eslint-disable */
/**
 * Wave 9 (ui-ux-verevon-gap.md §19): Verevon agent embed widget.
 *
 * Usage on a customer site:
 *
 *   <script
 *     src="https://verevon.example.com/embed.js"
 *     data-verevon-agent="agent_abc123"
 *     data-verevon-secret="<publicSecret from agent>"
 *     defer
 *   ></script>
 *
 * What it does:
 *   1. Reads agent id + secret from its own <script> data-* attrs.
 *   2. Calls `/api/embed/{agentId}/config?secret=...` to fetch
 *      branding (name, greeting, theme).
 *   3. Renders a floating chat bubble (bottom-right) + an expandable
 *      conversation panel — all shadow-DOM-isolated so customer CSS
 *      can't bleed into our markup.
 *   4. Each user message → `/api/embed/{agentId}/stream` (SSE), append
 *      the assistant reply.
 *
 * No framework, no build step — vanilla JS so it's a single network
 * request the browser can cache aggressively. Sized to <8KB minified.
 *
 * Visitor identity: random UUID per browser, persisted in
 * localStorage under `verevon_visitor_{agentId}`. Survives reload,
 * scoped per agent so the same browser talking to two different
 * agents keeps two distinct conversation threads.
 */
(function () {
  'use strict';

  // ── Find own <script> tag + read config attrs ──────────────────────
  var scripts = document.getElementsByTagName('script');
  var selfScript = null;
  for (var i = scripts.length - 1; i >= 0; i -= 1) {
    var s = scripts[i];
    if (
      s.src &&
      (s.src.indexOf('/embed.js') !== -1 || s.src.indexOf('embed.js?') !== -1) &&
      s.getAttribute('data-verevon-agent')
    ) {
      selfScript = s;
      break;
    }
  }
  if (!selfScript) {
    console.warn('[verevon] embed.js loaded but no agent attrs found');
    return;
  }

  var AGENT_ID = selfScript.getAttribute('data-verevon-agent');
  var SECRET = selfScript.getAttribute('data-verevon-secret');
  // Derive the verevon host from the script src so the customer doesn't
  // also have to set a base URL.
  var SCRIPT_SRC_URL = new URL(selfScript.src, document.baseURI);
  var BASE_URL = SCRIPT_SRC_URL.origin;

  if (!AGENT_ID || !SECRET) {
    console.warn('[verevon] embed.js missing data-verevon-agent or data-verevon-secret');
    return;
  }

  // ── Visitor identity (persisted per agent) ─────────────────────────
  var VISITOR_KEY = 'verevon_visitor_' + AGENT_ID;
  var visitorId = '';
  try {
    visitorId = localStorage.getItem(VISITOR_KEY) || '';
  } catch (_) {
    /* localStorage may be blocked; use ephemeral */
  }
  if (!visitorId) {
    visitorId =
      'v_' +
      Date.now().toString(36) +
      '_' +
      Math.random().toString(36).slice(2, 10);
    try { localStorage.setItem(VISITOR_KEY, visitorId); } catch (_) {}
  }

  // ── Fetch config, then mount UI ────────────────────────────────────
  fetch(
    BASE_URL + '/api/embed/' + encodeURIComponent(AGENT_ID) + '/config?secret=' + encodeURIComponent(SECRET),
    { cache: 'force-cache' }
  )
    .then(function (r) {
      if (!r.ok) throw new Error('config ' + r.status);
      return r.json();
    })
    .then(function (config) { mount(config); })
    .catch(function (err) {
      // Silent on the customer's page — the widget is non-essential
      // chrome; we never want to crash their site if the agent is
      // disabled or our backend is down.
      console.warn('[verevon] embed config unavailable:', err && err.message);
    });

  // ── Render bubble + panel inside a shadow DOM ──────────────────────
  function mount(config) {
    var host = document.createElement('div');
    host.setAttribute('data-verevon-embed', AGENT_ID);
    host.style.cssText = 'all:initial;position:fixed;bottom:24px;right:24px;z-index:2147483647';
    document.body.appendChild(host);
    var root = host.attachShadow({ mode: 'open' });

    var accent = (config.theme && config.theme.accentColor) || '#1A1A1A';
    var buttonLabel = (config.theme && config.theme.buttonLabel) || 'Chat';
    var welcome =
      (config.theme && config.theme.welcomeMessage) ||
      config.greeting ||
      'Hi! How can I help?';

    root.innerHTML =
      '<style>' +
      ':host{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;color:#111}' +
      '.btn{cursor:pointer;background:' + accent + ';color:#fff;border:none;border-radius:9999px;padding:12px 18px;font-size:14px;font-weight:600;box-shadow:0 4px 12px rgba(0,0,0,.18);display:flex;align-items:center;gap:8px}' +
      '.btn:hover{opacity:.92}' +
      '.panel{display:none;position:fixed;bottom:88px;right:24px;width:360px;height:520px;background:#fff;border-radius:14px;box-shadow:0 12px 40px rgba(0,0,0,.18);overflow:hidden;flex-direction:column;border:1px solid #E5E7EB}' +
      '.panel.open{display:flex}' +
      '.header{padding:14px 16px;background:' + accent + ';color:#fff;font-weight:600;display:flex;justify-content:space-between;align-items:center}' +
      '.close{cursor:pointer;background:transparent;border:none;color:#fff;font-size:18px;line-height:1}' +
      '.body{flex:1;overflow-y:auto;padding:16px;background:#fafafa}' +
      '.row{display:flex;margin-bottom:10px}' +
      '.row.user{justify-content:flex-end}' +
      '.bubble{max-width:80%;padding:8px 12px;border-radius:14px;font-size:13px;line-height:1.4;white-space:pre-wrap;word-wrap:break-word}' +
      '.row.asst .bubble{background:#fff;color:#111;border:1px solid #E5E7EB;border-top-left-radius:4px}' +
      '.row.user .bubble{background:' + accent + ';color:#fff;border-top-right-radius:4px}' +
      '.input{display:flex;gap:6px;padding:10px;border-top:1px solid #E5E7EB;background:#fff}' +
      '.input input{flex:1;border:1px solid #E5E7EB;border-radius:9999px;padding:8px 14px;font-size:13px;outline:none}' +
      '.input input:focus{border-color:' + accent + '}' +
      '.input button{background:' + accent + ';color:#fff;border:none;border-radius:9999px;padding:8px 16px;font-size:13px;cursor:pointer}' +
      '.input button:disabled{opacity:.5;cursor:not-allowed}' +
      '.typing{display:inline-flex;gap:4px}.dot{width:6px;height:6px;border-radius:50%;background:#999;animation:blk 1.2s infinite}' +
      '.dot:nth-child(2){animation-delay:.2s}.dot:nth-child(3){animation-delay:.4s}' +
      '@keyframes blk{0%,80%,100%{opacity:.3}40%{opacity:1}}' +
      '</style>' +
      '<button class="btn" id="btn">' + escapeHtml(buttonLabel) + '</button>' +
      '<div class="panel" id="panel">' +
      '<div class="header"><span>' + escapeHtml(config.name || 'Chat') + '</span><button class="close" id="close">×</button></div>' +
      '<div class="body" id="body"></div>' +
      '<div class="input"><input id="msg" placeholder="Type a message…" /><button id="send">Send</button></div>' +
      '</div>';

    var bodyEl = root.getElementById('body');
    var msgEl = root.getElementById('msg');
    var sendEl = root.getElementById('send');
    var panelEl = root.getElementById('panel');
    var btnEl = root.getElementById('btn');
    var closeEl = root.getElementById('close');

    appendMessage('asst', welcome);

    btnEl.addEventListener('click', function () { panelEl.classList.add('open'); msgEl.focus(); });
    closeEl.addEventListener('click', function () { panelEl.classList.remove('open'); });

    function appendMessage(role, text) {
      var row = document.createElement('div');
      row.className = 'row ' + role;
      var bubble = document.createElement('div');
      bubble.className = 'bubble';
      bubble.textContent = text;
      row.appendChild(bubble);
      bodyEl.appendChild(row);
      bodyEl.scrollTop = bodyEl.scrollHeight;
      return bubble;
    }
    function appendTyping() {
      var row = document.createElement('div');
      row.className = 'row asst';
      var bubble = document.createElement('div');
      bubble.className = 'bubble';
      bubble.innerHTML = '<span class="typing"><span class="dot"></span><span class="dot"></span><span class="dot"></span></span>';
      row.appendChild(bubble);
      bodyEl.appendChild(row);
      bodyEl.scrollTop = bodyEl.scrollHeight;
      return { row: row, bubble: bubble };
    }

    async function send() {
      var text = (msgEl.value || '').trim();
      if (!text) return;
      msgEl.value = '';
      sendEl.disabled = true;
      appendMessage('user', text);
      var typing = appendTyping();

      try {
        var res = await fetch(
          BASE_URL + '/api/embed/' + encodeURIComponent(AGENT_ID) + '/stream',
          {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ message: text, secret: SECRET, visitorId: visitorId }),
          }
        );
        if (!res.ok || !res.body) throw new Error('HTTP ' + res.status);

        var reader = res.body.getReader();
        var decoder = new TextDecoder();
        var buf = '';
        var content = '';

        while (true) {
          var chunk = await reader.read();
          if (chunk.done) break;
          buf += decoder.decode(chunk.value, { stream: true });

          var idx;
          while ((idx = buf.indexOf('\n\n')) !== -1) {
            var line = buf.slice(0, idx).trim();
            buf = buf.slice(idx + 2);
            if (!line.indexOf('data: ') === 0) continue;
            var raw = line.slice(6);
            if (!raw) continue;
            try {
              var ev = JSON.parse(raw);
              if (ev.type === 'answer_chunk' && typeof ev.content === 'string') {
                content = ev.content;
                typing.bubble.textContent = content;
                bodyEl.scrollTop = bodyEl.scrollHeight;
              } else if (ev.type === 'error') {
                content = 'Sorry — something went wrong.';
                typing.bubble.textContent = content;
              }
            } catch (_) {}
          }
        }
        if (!content) typing.bubble.textContent = '…';
      } catch (err) {
        typing.bubble.textContent = 'Could not reach the agent. Try again later.';
      } finally {
        sendEl.disabled = false;
        msgEl.focus();
      }
    }

    sendEl.addEventListener('click', send);
    msgEl.addEventListener('keydown', function (e) {
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        send();
      }
    });
  }

  function escapeHtml(s) {
    return String(s).replace(/[&<>"']/g, function (c) {
      return ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c];
    });
  }
})();
