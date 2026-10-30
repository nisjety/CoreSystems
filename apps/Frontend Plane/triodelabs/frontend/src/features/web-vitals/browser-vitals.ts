import { safeVitalsSample, vitalsPage } from "./policy";
import {
  DEVELOPMENT_CONSENT_EVENT,
  readDevelopmentConsent,
  privacySignal,
} from "./development-consent";

type MetricInput = { name: string; value: number };
type VitalsLibrary = {
  onLCP(callback: (metric: MetricInput) => void): void;
  onINP(callback: (metric: MetricInput) => void): void;
  onCLS(callback: (metric: MetricInput) => void): void;
};
type VitalsWindow = Window & {
  Cookiebot?: { consent?: { statistics?: boolean } };
};
type Options = Readonly<{ enabled: boolean; development: boolean }>;
type Runtime = { attach(): () => void };
const runtimes = new WeakMap<Window, Runtime>();

function createRuntime(
  win: VitalsWindow,
  options: Options,
  load: () => Promise<VitalsLibrary>,
  send: typeof fetch,
): Runtime {
  const initialUrl = win.location.href;
  const development =
    options.development &&
    process.env.NODE_ENV === "development" &&
    process.env.NEXT_PUBLIC_WEB_VITALS_ENABLED === "true";
  const path = vitalsPage(initialUrl, development);
  const statistics = () =>
    readDevelopmentConsent(win, development) ??
    win.Cookiebot?.consent?.statistics === true;
  const viewport = win.innerWidth < 768 ? "narrow" : "wide";
  let started = false,
    stopped = false,
    attached = 0,
    observing = false;
  let library: VitalsLibrary | undefined;
  let sampleId = "";
  let sequences: Record<string, number> = {};
  const allowed = () =>
    attached > 0 &&
    !stopped &&
    statistics() &&
    !privacySignal(win) &&
    win.location.href === initialUrl &&
    path !== null;

  const report = (metric: MetricInput) => {
    if (!allowed()) return;
    const sequence = (sequences[metric.name] ?? 0) + 1;
    const sample = safeVitalsSample({
      sampleId,
      sequence,
      metric: metric.name,
      value: metric.value,
      viewport,
      path,
    });
    if (!sample) return;
    sequences = { ...sequences, [metric.name]: sequence };
    // No metric.entries/attribution, location, referrer, cookies or persistent ID.
    void send("/api/web-vitals", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      credentials: "omit",
      referrerPolicy: "no-referrer",
      cache: "no-store",
      keepalive: true,
      redirect: "error",
      body: JSON.stringify(sample),
    })
      .then((response) => {
        if (!response.ok)
          win.dispatchEvent(new Event("triodelab:web-vitals-unavailable"));
      })
      .catch(() => {
        win.dispatchEvent(new Event("triodelab:web-vitals-unavailable"));
      });
  };
  const reconcile = () => {
    if (win.location.href !== initialUrl) stopped = true;
    if (!allowed()) {
      if (started && !statistics() && !stopped) {
        stopped = true;
        // web-vitals has no unsubscribe API. A fresh document drops observers.
        win.location.reload();
      }
      return;
    }
    if (library && !observing) {
      observing = true;
      library.onLCP(report);
      library.onINP(report);
      library.onCLS(report);
    }
    if (started || !win.crypto?.randomUUID) return;
    started = true;
    sampleId = win.crypto.randomUUID();
    void load()
      .then((value) => {
        library = value;
        reconcile();
      })
      .catch(() => {
        stopped = true;
      });
  };
  const sensitive = (event: Event) => {
    if (
      typeof Element !== "undefined" &&
      event.target instanceof Element &&
      event.target.closest(
        "input, textarea, select, form, [contenteditable=true], [aria-label='Aktiver chat fra Agenci']",
      ) &&
      !event.target.closest("#CybotCookiebotDialog, #CookiebotWidget")
    )
      stopped = true;
  };
  const restored = (event: Event) => {
    if ((event as PageTransitionEvent).persisted && allowed()) {
      sampleId = win.crypto.randomUUID();
      sequences = {};
    }
  };
  const consentEvents = [
    "CookiebotOnConsentReady",
    "CookiebotOnAccept",
    "CookiebotOnDecline",
    DEVELOPMENT_CONSENT_EVENT,
  ];
  const privateEvents = [
    "pointerdown",
    "keydown",
    "focusin",
    "input",
    "submit",
  ];
  let restoreHistory = () => {};
  const guardHistory = () => {
    const originals = {
      pushState: win.history.pushState,
      replaceState: win.history.replaceState,
    };
    const wrappers = Object.fromEntries(
      (["pushState", "replaceState"] as const).map((name) => [
        name,
        function (data: unknown, unused: string, url?: string | URL | null) {
          originals[name].call(win.history, data, unused, url);
          reconcile();
        },
      ]),
    ) as typeof originals;
    win.history.pushState = wrappers.pushState;
    win.history.replaceState = wrappers.replaceState;
    restoreHistory = () => {
      for (const name of ["pushState", "replaceState"] as const) {
        if (win.history[name] === wrappers[name])
          win.history[name] = originals[name];
      }
    };
  };
  return {
    attach() {
      attached += 1;
      if (attached === 1) {
        guardHistory();
        consentEvents.forEach((name) => win.addEventListener(name, reconcile));
        privateEvents.forEach((name) =>
          win.addEventListener(name, sensitive, true),
        );
        win.addEventListener("pageshow", restored);
        win.addEventListener("popstate", reconcile, true);
        win.addEventListener("hashchange", reconcile, true);
      }
      reconcile();
      let detached = false;
      return () => {
        if (detached) return;
        detached = true;
        attached -= 1;
        if (attached > 0) return;
        restoreHistory();
        consentEvents.forEach((name) =>
          win.removeEventListener(name, reconcile),
        );
        privateEvents.forEach((name) =>
          win.removeEventListener(name, sensitive, true),
        );
        win.removeEventListener("pageshow", restored);
        win.removeEventListener("popstate", reconcile, true);
        win.removeEventListener("hashchange", reconcile, true);
      };
    },
  };
}

/** One document runtime; StrictMode/route effects never install duplicate observers. */
export function installWebVitals(
  win: Window,
  options: Options,
  load: () => Promise<VitalsLibrary> = () => import("web-vitals"),
  send: typeof fetch = fetch,
) {
  if (!options.enabled) return () => {};
  const runtime = runtimes.get(win) ?? createRuntime(win, options, load, send);
  runtimes.set(win, runtime);
  return runtime.attach();
}
