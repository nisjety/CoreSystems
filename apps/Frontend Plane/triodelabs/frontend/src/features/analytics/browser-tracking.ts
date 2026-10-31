import {
  createConsentTracking,
  type ConsentTracking,
  type TrackingConfig,
} from "./consent-tracking";

type CommandQueue = ((...args: unknown[]) => void) & { q?: unknown[][] };
type TrackingWindow = Window & {
  Cookiebot?: { consent?: { statistics?: boolean } };
  dataLayer?: unknown[];
  gtag?: CommandQueue;
  clarity?: CommandQueue;
};

const runtimes = new WeakMap<Window, ConsentTracking>();

/** Clear only this installation's first-party analytics cookies, never CMP/session cookies. */
function clearAnalyticsCookies(doc: Document, hostname: string) {
  const names = doc.cookie
    .split(";")
    .map((cookie) => cookie.split("=", 1)[0].trim())
    .filter((name) => /^(_ga(?:_[A-Z0-9]+)?|_clck|_clsk)$/.test(name));
  for (const name of names) {
    for (const domain of [
      "",
      `; Domain=${hostname}`,
      "; Domain=triodelab.no",
    ]) {
      doc.cookie = `${name}=; Max-Age=0; Path=/${domain}; SameSite=Lax; Secure`;
    }
  }
}

function runtimeFor(
  win: TrackingWindow,
  doc: Document,
  config: TrackingConfig,
) {
  const existing = runtimes.get(win);
  if (existing) return existing;
  const runtime = createConsentTracking(config, {
    url: () => win.location.href,
    referrer: () => doc.referrer,
    statistics: () => win.Cookiebot?.consent?.statistics === true,
    privacySignal: () =>
      win.navigator.doNotTrack === "1" ||
      (win.navigator as Navigator & { globalPrivacyControl?: boolean })
        .globalPrivacyControl === true,
    loadScript(id, src) {
      if (doc.getElementById(id)) return;
      const script = doc.createElement("script");
      script.id = id;
      script.src = src;
      script.async = true;
      script.referrerPolicy = "no-referrer";
      script.setAttribute("data-cookieconsent", "statistics");
      script.addEventListener(
        "error",
        () => script.setAttribute("data-tracking-state", "unavailable"),
        { once: true },
      );
      doc.head.appendChild(script);
    },
    google(...args) {
      win.dataLayer ??= [];
      win.gtag ??= function () {
        // Google expects Arguments entries rather than a custom event envelope.
        // eslint-disable-next-line prefer-rest-params -- gtag's documented queue format is Arguments.
        win.dataLayer?.push(arguments);
      };
      win.gtag(...args);
    },
    clarity(...args) {
      win.clarity ??= Object.assign(
        (...values: unknown[]) => {
          win.clarity?.q?.push(values);
        },
        { q: [] as unknown[][] },
      );
      win.clarity(...args);
    },
    disableGoogle(id, disabled) {
      Object.assign(win, { [`ga-disable-${id}`]: disabled });
    },
    clearCookies: () => clearAnalyticsCookies(doc, win.location.hostname),
    reload: () => win.location.reload(),
  });
  runtimes.set(win, runtime);
  return runtime;
}

/** One runtime per document; StrictMode remounts only reattach the event listeners. */
export function installBrowserTracking(
  win: TrackingWindow,
  doc: Document,
  config: TrackingConfig,
) {
  if (!config.measurementId && !config.clarityProjectId) return () => {};
  const runtime = runtimeFor(win, doc, config);
  const reconcile = () => runtime.reconcile();
  const events = [
    "CookiebotOnConsentReady",
    "CookiebotOnAccept",
    "CookiebotOnDecline",
  ];
  events.forEach((event) => win.addEventListener(event, reconcile));

  // Do not leave long-lived third-party observers running into private SPA pages.
  // Stop synchronously, then use a fresh document for every changed history URL.
  const push = win.history.pushState;
  const replace = win.history.replaceState;
  const guarded = (original: History["pushState"]): History["pushState"] =>
    function (data, unused, url) {
      const target =
        url == null ? win.location.href : new URL(url, win.location.href).href;
      // Keep native same-origin validation (and its SecurityError) for invalid history calls.
      if (new URL(target).origin !== win.location.origin) {
        original.call(win.history, data, unused, url);
        return;
      }
      if (runtime.beforeNavigation(target)) {
        win.location.assign(target);
        return;
      }
      original.call(win.history, data, unused, url);
    };
  const guardedPush = guarded(push);
  const guardedReplace = guarded(replace);
  win.history.pushState = guardedPush;
  win.history.replaceState = guardedReplace;
  const pop = () => {
    // popstate has already changed the URL; stop before the router renders it.
    if (runtime.beforeNavigation("document-history-transition"))
      win.location.reload();
  };
  win.addEventListener("popstate", pop, { capture: true });
  win.addEventListener("hashchange", pop, { capture: true });

  const interaction = (event: Event) => {
    const target = event.target;
    if (!(target instanceof Element)) return;
    if (target.closest("#CybotCookiebotDialog, #CookiebotWidget")) {
      // CMP controls contain no site form input. Allow initial opt-in, but stop
      // any already-running tracker before automatic form metadata is inspected.
      runtime.sensitiveInteraction(false);
      return;
    }
    if (
      target.closest(
        "input, textarea, select, form, [contenteditable=true], [aria-label='Aktiver chat fra Agenci']",
      )
    ) {
      // Includes GA4: Enhanced Measurement can inspect form/chat metadata.
      runtime.sensitiveInteraction();
    }
    const anchor = target.closest("a[href]") as HTMLAnchorElement | null;
    if (!anchor) return;
    const modified =
      (event as MouseEvent).ctrlKey ||
      (event as MouseEvent).metaKey ||
      (event as MouseEvent).shiftKey ||
      (event as MouseEvent).altKey;
    const otherTarget = anchor.target !== "" && anchor.target !== "_self";
    let url: URL;
    try {
      url = new URL(anchor.href, win.location.href);
    } catch {
      runtime.sensitiveInteraction();
      return;
    }
    if (
      url.origin !== win.location.origin ||
      url.search ||
      url.hash ||
      url.username ||
      url.password ||
      anchor.hasAttribute("download") ||
      otherTarget ||
      modified
    ) {
      // Disable before any vendor's outbound/download listeners, including new-tab
      // and modifier clicks. Preserve the browser's native navigation behavior.
      runtime.sensitiveInteraction();
    }
    if (
      event.type !== "click" ||
      otherTarget ||
      anchor.hasAttribute("download") ||
      modified
    )
      return;
    if (url.origin !== win.location.origin) return;
    if (runtime.beforeNavigation(url.href)) {
      event.preventDefault();
      event.stopPropagation();
      win.location.assign(url.href);
    }
  };
  const interactionEvents = [
    "pointerdown",
    "mousedown",
    "click",
    "auxclick",
    "focusin",
    "keydown",
    "input",
    "submit",
  ];
  // Attach to window before loading Google: earlier capture-phase guards run
  // before the vendor's later window/document listeners in normal event dispatch.
  interactionEvents.forEach((event) =>
    win.addEventListener(event, interaction, { capture: true }),
  );
  reconcile();

  return () => {
    events.forEach((event) => win.removeEventListener(event, reconcile));
    interactionEvents.forEach((event) =>
      win.removeEventListener(event, interaction, { capture: true }),
    );
    win.removeEventListener("popstate", pop, { capture: true });
    win.removeEventListener("hashchange", pop, { capture: true });
    if (win.history.pushState === guardedPush) win.history.pushState = push;
    if (win.history.replaceState === guardedReplace)
      win.history.replaceState = replace;
  };
}
