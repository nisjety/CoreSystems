/*
 * Boot watchdog (WS07-S2). Classic script, served no-cache by nginx, loaded from
 * index.html before the module script (CSP forbids inline script).
 *
 * If the app has not set window.__verevonBoot === true 15 s after load, the
 * bundle most likely failed to load or crashed before the first render:
 *   1. fetch /version.json; when its build_id differs from
 *      <meta name="verevon-build">, the tab is on a stale build: reload once,
 *      guarded by the same sessionStorage key the SPA's chunk recovery uses;
 *   2. otherwise paint a plain-DOM card (Reload) and beacon a content-free
 *      {kind:'watchdog', build} to /api/v1/client-diagnostics.
 * Every storage and network access is guarded; nothing here may throw.
 */
(function () {
  'use strict'
  var TIMEOUT_MS = 15000
  var GUARD_KEY = 'verevon.chunk-reload.v1' // keep equal to CHUNK_RELOAD_KEY in src/app/chunk-recovery.ts
  var GUARD_WINDOW_MS = 30000
  var LOCALE_KEY = 'verevon.locale'

  function metaBuild() {
    try {
      var el = document.querySelector('meta[name="verevon-build"]')
      return (el && el.getAttribute('content')) || ''
    } catch (e) {
      return ''
    }
  }

  function reloadAllowed() {
    try {
      var raw = window.sessionStorage.getItem(GUARD_KEY)
      if (raw !== null) {
        var at = Number(raw)
        if (isFinite(at) && Date.now() - at < GUARD_WINDOW_MS) return false
      }
      window.sessionStorage.setItem(GUARD_KEY, String(Date.now()))
      return true
    } catch (e) {
      return false // without the marker a reload could loop forever
    }
  }

  function isNorwegian() {
    try {
      var stored = window.localStorage.getItem(LOCALE_KEY)
      if (stored) return stored.toLowerCase().indexOf('en') !== 0
    } catch (e) {
      /* fall through */
    }
    return String(navigator.language || 'nb').toLowerCase().indexOf('en') !== 0
  }

  function beacon(build) {
    try {
      var payload = JSON.stringify({ kind: 'watchdog', build: String(build || '').slice(0, 12) })
      var body = new Blob([payload], { type: 'application/json' })
      if (navigator.sendBeacon) navigator.sendBeacon('/api/v1/client-diagnostics', body)
    } catch (e) {
      /* diagnostics are best effort */
    }
  }

  function paintCard(build) {
    try {
      var root = document.getElementById('root') || document.body
      if (!root || document.getElementById('verevon-boot-failure')) return
      var no = isNorwegian()
      var card = document.createElement('div')
      card.id = 'verevon-boot-failure'
      card.setAttribute('role', 'alert')
      card.style.cssText =
        'max-width:520px;margin:18vh auto 0;padding:20px 22px;border:1px solid #e7e7e5;border-radius:12px;' +
        'font-family:system-ui,sans-serif;color:#1a1a1a;background:#fff'
      var h = document.createElement('h2')
      h.style.cssText = 'margin:0 0 8px;font-size:16px;font-weight:600'
      h.textContent = no ? 'Appen startet ikke' : 'The app did not start'
      var p = document.createElement('p')
      p.style.cssText = 'margin:0 0 12px;font-size:14px;line-height:1.5;color:#66615b'
      p.textContent = no
        ? 'Last siden på nytt. Hvis problemet fortsetter, kontakt administratoren.'
        : 'Reload the page. If the problem continues, contact your administrator.'
      var b = document.createElement('button')
      b.type = 'button'
      b.style.cssText = 'padding:6px 14px;font-size:14px;cursor:pointer'
      b.textContent = no ? 'Last på nytt' : 'Reload'
      b.addEventListener('click', function () {
        window.location.reload()
      })
      card.appendChild(h)
      card.appendChild(p)
      card.appendChild(b)
      root.appendChild(card)
    } catch (e) {
      /* nothing left to try */
    }
    beacon(build)
  }

  function onTimeout() {
    if (window.__verevonBoot === true) return
    var mine = metaBuild()
    var fail = function () {
      if (window.__verevonBoot !== true) paintCard(mine)
    }
    try {
      fetch('/version.json', { cache: 'no-store' })
        .then(function (res) {
          var type = res.headers.get('content-type') || ''
          // The SPA fallback answers index.html here until nginx serves the file.
          if (!res.ok || type.indexOf('json') === -1) throw new Error('not-json')
          return res.json()
        })
        .then(function (info) {
          var live = info && typeof info.build_id === 'string' ? info.build_id : ''
          if (window.__verevonBoot === true) return
          if (live && mine && mine !== 'dev' && live !== mine && reloadAllowed()) {
            window.location.reload()
            return
          }
          fail()
        })
        .catch(fail)
    } catch (e) {
      fail()
    }
  }

  window.setTimeout(onTimeout, TIMEOUT_MS)
})()
