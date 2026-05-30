//! Browser-driver pre-scripts. JavaScript blobs injected via CDP's
//! `Page.addScriptToEvaluateOnNewDocument` so they run BEFORE any page
//! JS on every navigation in the session. Drivers that don't expose
//! pre-script injection (kernel today) can fall back to running these
//! after document-ready via the existing `evaluate` API — strictly
//! less effective but better than nothing.
//!
//! Two scripts ship by default:
//!
//! 1. **`STEALTH_SCRIPT`** — masks the cheapest headless-browser tells.
//!    Not a replacement for a full puppeteer-stealth port; the goal is
//!    "don't fail Cloudflare's free tier" not "evade enterprise bot
//!    detection". Covers `navigator.webdriver`, the `window.chrome`
//!    object, the plugins array, and language fingerprinting.
//!
//! 2. **`CONSENT_DISMISS_SCRIPT`** — auto-clicks common cookie/consent
//!    overlay accept buttons (OneTrust, Cookiebot, CookieYes,
//!    Quantcast, TrustArc, Didomi, and a generic English/Norwegian/
//!    Swedish/German fallback list). Most European publisher sites
//!    hide content behind these overlays; without dismissal,
//!    readability extracts the consent dialog as the page's main
//!    content. The script runs on a short interval for ~6 seconds
//!    after navigation so it catches overlays that mount late.
//!
//! Both scripts are idempotent and safe to inject on every page —
//! they only act when the relevant DOM markers are present.

/// JS to neutralise headless-Chrome fingerprint signals across the
/// surfaces bot-detection products actually probe. Inject via
/// `Page.addScriptToEvaluateOnNewDocument` so it runs before any page
/// script reads `navigator.webdriver` / measures canvas / queries
/// WebGL / synthesises AudioContext output.
///
/// Coverage tiers, cheapest first:
/// - **navigator**: `webdriver`, `plugins`, `languages`, `permissions`,
///   `chrome` shim
/// - **canvas**: per-context jitter on `toDataURL` / `getImageData` so
///   the canvas fingerprint changes per session (defeats static-hash
///   bot scoring)
/// - **WebGL**: vendor/renderer spoof to "Intel Open Source Technology
///   Center" / "Mesa DRI Intel(R) HD Graphics 4000" (a real, common
///   pair that doesn't match the typical Chrome-headless signature
///   "Google Inc. (Google) / ANGLE (Google, Vulkan ..., SwiftShader
///   Device)")
/// - **AudioContext**: jitter the OfflineAudioContext frequency-bin
///   output so the audio-fingerprint differs from the deterministic
///   value headless produces
/// - **screen / window**: align ratios to a plausible 1440x900 laptop
///   instead of headless's odd 800x600
/// - **iframe.contentWindow.chrome**: real Chrome keeps the shim in
///   iframes; headless drops it
pub const STEALTH_SCRIPT: &str = r#"
(() => {
  // ============================================================
  // navigator.webdriver — primary headless tell
  // ============================================================
  try {
    Object.defineProperty(navigator, 'webdriver', {
      get: () => undefined,
      configurable: true,
    });
  } catch (_) {}

  // ============================================================
  // window.chrome — absent in headless; presence is a quick check
  // ============================================================
  try {
    if (!window.chrome) {
      window.chrome = {
        runtime: {},
        app: { isInstalled: false, InstallState: { DISABLED: 'disabled', INSTALLED: 'installed', NOT_INSTALLED: 'not_installed' } },
        csi: () => undefined,
        loadTimes: () => undefined,
      };
    }
  } catch (_) {}

  // ============================================================
  // navigator.plugins / mimeTypes — real Chrome ships 3-4
  // ============================================================
  try {
    if (navigator.plugins.length === 0) {
      const fake = [
        { name: 'PDF Viewer', filename: 'internal-pdf-viewer', description: 'Portable Document Format' },
        { name: 'Chrome PDF Viewer', filename: 'internal-pdf-viewer', description: '' },
        { name: 'Chromium PDF Viewer', filename: 'internal-pdf-viewer', description: '' },
        { name: 'Microsoft Edge PDF Viewer', filename: 'internal-pdf-viewer', description: '' },
        { name: 'WebKit built-in PDF', filename: 'internal-pdf-viewer', description: '' },
      ];
      fake.length = fake.length; // pretend PluginArray-ish
      Object.defineProperty(navigator, 'plugins', {
        get: () => fake,
        configurable: true,
      });
    }
  } catch (_) {}

  // ============================================================
  // navigator.languages — sane locale chain default
  // ============================================================
  try {
    if (!navigator.languages || navigator.languages.length < 2) {
      Object.defineProperty(navigator, 'languages', {
        get: () => ['en-US', 'en'],
        configurable: true,
      });
    }
  } catch (_) {}

  // ============================================================
  // navigator.permissions — fix the notifications discrepancy
  // ============================================================
  try {
    const orig = navigator.permissions && navigator.permissions.query;
    if (orig) {
      navigator.permissions.query = (params) => {
        if (params && params.name === 'notifications') {
          return Promise.resolve({ state: Notification.permission });
        }
        return orig.call(navigator.permissions, params);
      };
    }
  } catch (_) {}

  // ============================================================
  // navigator.platform / hardwareConcurrency / deviceMemory —
  // align to a common laptop profile
  // ============================================================
  try {
    Object.defineProperty(navigator, 'hardwareConcurrency', { get: () => 8, configurable: true });
  } catch (_) {}
  try {
    Object.defineProperty(navigator, 'deviceMemory', { get: () => 8, configurable: true });
  } catch (_) {}

  // ============================================================
  // screen / window — headless ships 800x600; users don't
  // ============================================================
  try {
    Object.defineProperty(screen, 'width',  { get: () => 1440, configurable: true });
    Object.defineProperty(screen, 'height', { get: () => 900,  configurable: true });
    Object.defineProperty(screen, 'availWidth',  { get: () => 1440, configurable: true });
    Object.defineProperty(screen, 'availHeight', { get: () => 860,  configurable: true });
    Object.defineProperty(screen, 'colorDepth',  { get: () => 24,   configurable: true });
    Object.defineProperty(screen, 'pixelDepth',  { get: () => 24,   configurable: true });
  } catch (_) {}

  // ============================================================
  // Canvas fingerprint: jitter `toDataURL` / `getImageData` by
  // ±1 in the LSB of every 4th pixel. Visually identical, makes
  // each session produce a different fingerprint hash. Pure
  // JS — no native dependencies.
  // ============================================================
  try {
    const seed = Math.floor(Math.random() * 1e9);
    const xorshift = (() => {
      let s = seed >>> 0;
      return () => {
        s ^= s << 13; s >>>= 0;
        s ^= s >>> 17;
        s ^= s << 5;  s >>>= 0;
        return s;
      };
    })();
    const origToDataURL = HTMLCanvasElement.prototype.toDataURL;
    HTMLCanvasElement.prototype.toDataURL = function (...args) {
      try {
        const ctx = this.getContext('2d');
        if (ctx && this.width > 0 && this.height > 0) {
          const img = ctx.getImageData(0, 0, this.width, this.height);
          for (let i = 0; i < img.data.length; i += 16) {
            // Flip the LSB of one channel every ~4 pixels.
            img.data[i] = img.data[i] ^ (xorshift() & 1);
          }
          ctx.putImageData(img, 0, 0);
        }
      } catch (_) {}
      return origToDataURL.apply(this, args);
    };
    const origGetImageData = CanvasRenderingContext2D.prototype.getImageData;
    CanvasRenderingContext2D.prototype.getImageData = function (...args) {
      const img = origGetImageData.apply(this, args);
      try {
        for (let i = 0; i < img.data.length; i += 16) {
          img.data[i] = img.data[i] ^ (xorshift() & 1);
        }
      } catch (_) {}
      return img;
    };
  } catch (_) {}

  // ============================================================
  // WebGL vendor / renderer — return a common Intel pair instead
  // of the SwiftShader / ANGLE headless fingerprint
  // ============================================================
  try {
    const origGetParameter = WebGLRenderingContext.prototype.getParameter;
    WebGLRenderingContext.prototype.getParameter = function (param) {
      // UNMASKED_VENDOR_WEBGL = 37445, UNMASKED_RENDERER_WEBGL = 37446
      if (param === 37445) return 'Intel Inc.';
      if (param === 37446) return 'Intel Iris OpenGL Engine';
      return origGetParameter.apply(this, [param]);
    };
    if (typeof WebGL2RenderingContext !== 'undefined') {
      const origGetParameter2 = WebGL2RenderingContext.prototype.getParameter;
      WebGL2RenderingContext.prototype.getParameter = function (param) {
        if (param === 37445) return 'Intel Inc.';
        if (param === 37446) return 'Intel Iris OpenGL Engine';
        return origGetParameter2.apply(this, [param]);
      };
    }
  } catch (_) {}

  // ============================================================
  // AudioContext fingerprint — jitter the offline-rendered output
  // by adding micro-noise to each sample of the first buffer pulled
  // out via getChannelData / copyFromChannel. The fingerprint hash
  // changes per session; audio that's actually played sounds the
  // same because the perturbation is sub-perceptual.
  // ============================================================
  try {
    const audioSeed = Math.floor(Math.random() * 1e9);
    let audioState = audioSeed >>> 0;
    const audioRand = () => {
      audioState ^= audioState << 13; audioState >>>= 0;
      audioState ^= audioState >>> 17;
      audioState ^= audioState << 5;  audioState >>>= 0;
      return (audioState / 0xffffffff) * 1e-7; // sub-audible noise
    };
    const origGetChannelData = AudioBuffer.prototype.getChannelData;
    AudioBuffer.prototype.getChannelData = function (...args) {
      const data = origGetChannelData.apply(this, args);
      for (let i = 0; i < data.length; i += 100) {
        data[i] += audioRand();
      }
      return data;
    };
  } catch (_) {}

  // ============================================================
  // iframe.contentWindow.chrome — propagate the shim into iframes
  // ============================================================
  try {
    const origCreateElement = document.createElement.bind(document);
    document.createElement = function (tagName, ...rest) {
      const el = origCreateElement(tagName, ...rest);
      if (typeof tagName === 'string' && tagName.toLowerCase() === 'iframe') {
        // When the iframe attaches its window, propagate window.chrome.
        Object.defineProperty(el, 'contentWindow', {
          get() {
            const w = HTMLIFrameElement.prototype.__lookupGetter__('contentWindow').call(this);
            try { if (w && !w.chrome) w.chrome = window.chrome; } catch (_) {}
            return w;
          },
          configurable: true,
        });
      }
      return el;
    };
  } catch (_) {}

  // ============================================================
  // Function.prototype.toString masking — many detectors check
  // whether overridden built-ins still report `[native code]`.
  // Patch toString so it returns the native shape for our hooks.
  // ============================================================
  try {
    const origToString = Function.prototype.toString;
    const nativeMarker = ' { [native code] }';
    const wrapped = new WeakSet();
    Function.prototype.toString = function () {
      if (wrapped.has(this)) {
        return 'function ' + (this.name || '') + '()' + nativeMarker;
      }
      return origToString.apply(this, arguments);
    };
    // Mark our visible overrides as wrapped — extend this list when
    // you patch a new function above.
    try { wrapped.add(HTMLCanvasElement.prototype.toDataURL); } catch (_) {}
    try { wrapped.add(CanvasRenderingContext2D.prototype.getImageData); } catch (_) {}
    try { wrapped.add(WebGLRenderingContext.prototype.getParameter); } catch (_) {}
    try { wrapped.add(AudioBuffer.prototype.getChannelData); } catch (_) {}
  } catch (_) {}
})();
"#;

/// JS to dismiss common cookie / GDPR consent overlays. Polls every
/// 250 ms for ~6 seconds (24 attempts) so it catches overlays that
/// mount on document-ready, on idle, or after a small animation
/// delay. Each match accepts (clicks the affirmative button); we
/// don't decline because some sites then hide content as a penalty.
pub const CONSENT_DISMISS_SCRIPT: &str = r#"
(() => {
  // Selectors for the affirmative "Accept" button across consent
  // vendors. Order: vendor-specific first (precise), then text-based
  // (less precise but covers custom implementations).
  const VENDOR_SELECTORS = [
    // OneTrust
    '#onetrust-accept-btn-handler',
    '#accept-recommended-btn-handler',
    // Cookiebot
    '#CybotCookiebotDialogBodyLevelButtonLevelOptinAllowAll',
    '#CybotCookiebotDialogBodyButtonAccept',
    // CookieYes
    '.cky-btn-accept',
    '[data-cky-tag="accept-button"]',
    // Didomi
    '#didomi-notice-agree-button',
    'button.didomi-components-button--accept',
    // Quantcast (TCF)
    'button.qc-cmp2-summary-buttons[mode="primary"]',
    // TrustArc
    '#truste-consent-button',
    // Sourcepoint
    'button.message-button[title="Accept"]',
    // Iubenda
    '.iubenda-cs-accept-btn',
    // Klaro
    '.klaro .cm-btn-accept-all',
    // Osano
    '.osano-cm-accept-all',
    // Termly
    '[data-tid="banner-accept"]',
    // Usercentrics
    '[data-testid="uc-accept-all-button"]',
    'button[data-testid="uc-deny-all-button"]', // some sites label accept this way
    // Tealium
    '#consent_blackbar button.cookieSettingsBtn-accept',
  ];

  // Text-fallback selectors (case-insensitive substring match on
  // button text). Multilingual. We only click buttons inside elements
  // that look like consent overlays — a `[role=dialog]`, `[aria-modal]`,
  // or an ancestor with a class containing `cookie`/`consent`/`gdpr`.
  const TEXT_PATTERNS = [
    /^accept all/i,
    /^accept cookies/i,
    /^accept and continue/i,
    /^i agree/i,
    /^agree/i,
    /^godta alle/i,        // Norwegian (bokmål)
    /^godta alt/i,
    /^aksepter/i,
    /^godkänn alla/i,      // Swedish
    /^acceptera alla/i,
    /^accepter alle/i,     // Danish
    /^alle akzeptieren/i,  // German
    /^alle zustimmen/i,
    /^tout accepter/i,     // French
    /^aceptar todo/i,      // Spanish
    /^accetta tutto/i,     // Italian
    /^alles accepteren/i,  // Dutch
  ];

  function isInConsentScope(el) {
    let cur = el;
    for (let depth = 0; cur && depth < 8; depth += 1) {
      try {
        const role = cur.getAttribute && cur.getAttribute('role');
        if (role === 'dialog' || role === 'alertdialog') return true;
        if (cur.getAttribute && cur.getAttribute('aria-modal') === 'true') return true;
        const cls = (cur.className || '').toString().toLowerCase();
        const id = (cur.id || '').toLowerCase();
        if (/cookie|consent|gdpr|privacy/.test(cls + ' ' + id)) return true;
      } catch (_) {}
      cur = cur.parentElement;
    }
    return false;
  }

  function tryDismiss() {
    // Vendor-specific selectors first — these are precise.
    for (const sel of VENDOR_SELECTORS) {
      try {
        const el = document.querySelector(sel);
        if (el && typeof el.click === 'function') {
          el.click();
          return true;
        }
      } catch (_) {}
    }
    // Text-based fallback. Walk buttons + role=button elements,
    // require them to be visually inside a consent-shaped overlay.
    const candidates = document.querySelectorAll(
      'button, [role="button"], a.button, input[type="button"]'
    );
    for (const el of candidates) {
      try {
        const text = (el.innerText || el.textContent || el.value || '').trim();
        if (!text) continue;
        if (!TEXT_PATTERNS.some((p) => p.test(text))) continue;
        if (!isInConsentScope(el)) continue;
        if (typeof el.click === 'function') {
          el.click();
          return true;
        }
      } catch (_) {}
    }
    return false;
  }

  let attempts = 0;
  const maxAttempts = 24; // ~6 seconds at 250 ms cadence
  const interval = setInterval(() => {
    attempts += 1;
    try {
      if (tryDismiss() || attempts >= maxAttempts) {
        clearInterval(interval);
      }
    } catch (_) {
      clearInterval(interval);
    }
  }, 250);

  // First attempt on the next tick — catches overlays that mount
  // synchronously on DOMContentLoaded.
  setTimeout(tryDismiss, 0);
})();
"#;

/// Convenience: the script a driver should inject on every new
/// document. Stealth first (must run before page JS reads its
/// fingerprint), then consent dismissal (can run anytime).
pub fn default_preamble() -> String {
    let mut s = String::with_capacity(STEALTH_SCRIPT.len() + CONSENT_DISMISS_SCRIPT.len() + 16);
    s.push_str(STEALTH_SCRIPT);
    s.push('\n');
    s.push_str(CONSENT_DISMISS_SCRIPT);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preamble_includes_both_scripts() {
        let pre = default_preamble();
        assert!(pre.contains("navigator.webdriver"));
        assert!(pre.contains("onetrust-accept-btn-handler"));
    }

    #[test]
    fn scripts_compile_as_valid_js_text() {
        // Sanity: the literal blocks balance their parens/braces.
        // Cheap heuristic — exact JS parsing would need a real parser.
        for s in &[STEALTH_SCRIPT, CONSENT_DISMISS_SCRIPT] {
            let opens = s.matches('{').count();
            let closes = s.matches('}').count();
            assert_eq!(opens, closes, "unbalanced braces in script:\n{s}");
            let parens_open = s.matches('(').count();
            let parens_close = s.matches(')').count();
            assert_eq!(parens_open, parens_close, "unbalanced parens in script");
        }
    }
}
