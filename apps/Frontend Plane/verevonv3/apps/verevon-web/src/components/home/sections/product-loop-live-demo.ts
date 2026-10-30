/**
 * Pure helpers of the stage 04 live demo, the site side of the iframe host.
 *
 * The demo is the static bundle served at /product-demo/index.html, built by the demo
 * package (see apps/verevon-web/plans/product-demo/PLAN_2026-10-11.md). The message
 * types below mirror src/demo/live/shared-types.ts in the SPA. They are repeated here,
 * because the site must not import from the SPA tree.
 *
 * The iframe is sandboxed with `allow-scripts` only, so its origin is opaque and its
 * messages report origin "null". The host therefore checks `event.source` against the
 * iframe's contentWindow and the `{ source: 'verevon-demo', v: 1 }` envelope, never the
 * origin.
 */

export const LIVE_DEMO_SANDBOX = "allow-scripts";
export const LIVE_DEMO_BAR_HEIGHT = 44;
export const LIVE_DEMO_READY_TIMEOUT_MS = 15_000;

const LIVE_DEMO_SOURCE = "verevon-demo";
const LIVE_DEMO_VERSION = 1;
const LIVE_DEMO_ERROR_TEXT_LIMIT = 200;

export type ProductLoopLiveDemo = {
  frameTitle: string;
  minAppHeight: number;
  minAppWidth: number;
  poster: string;
  posterAlt: string;
  posterMobile: string;
  src: string;
};

export const PRODUCT_LOOP_LIVE_DEMO: ProductLoopLiveDemo = {
  frameTitle: "Verevon-demo: dashbordet med fiktive data for Fjordform AS",
  minAppHeight: 640,
  minAppWidth: 1100,
  poster: "/product-demo/poster-desktop.webp",
  posterAlt: "Forhåndsvisning av dashbordet med fiktive data for Fjordform AS.",
  posterMobile: "/product-demo/poster-phone.webp",
  src: "/product-demo/index.html",
};

export type DemoToSiteMessage =
  | { source: "verevon-demo"; v: 1; type: "ready" }
  | { source: "verevon-demo"; v: 1; type: "escape" }
  | { source: "verevon-demo"; v: 1; type: "error"; message: string };

export type SiteToDemoMessage =
  | { source: "verevon-demo"; v: 1; type: "activate" }
  | { source: "verevon-demo"; v: 1; type: "deactivate" }
  | { source: "verevon-demo"; v: 1; type: "pause" }
  | { source: "verevon-demo"; v: 1; type: "resume" }
  | { source: "verevon-demo"; v: 1; type: "reset" };

export type LiveDemoState =
  | "idle"
  | "deferred"
  | "loading"
  | "ready"
  | "active"
  | "failed";

export type LiveDemoEvent =
  | { type: "near"; deferred: boolean }
  | { type: "request-load" }
  | { type: "reload" }
  | { type: "ready" }
  | { type: "timeout" }
  | { type: "error" }
  | { type: "activate" }
  | { type: "release" };

/**
 * idle -> loading (or deferred when Save-Data or a slow connection), deferred or failed ->
 * loading on request, loading -> ready on the demo's ready message, ready <-> active on
 * activation and release, loaded -> loading on a reload (a reset of the demo), and
 * loading -> failed on timeout or on an error. A loaded demo ignores errors: the demo also
 * reports benign notices as errors, and it keeps running. Any other event leaves the state
 * as it is.
 */
export function reduceLiveDemo(
  state: LiveDemoState,
  event: LiveDemoEvent,
): LiveDemoState {
  switch (event.type) {
    case "near":
      return state === "idle" ? (event.deferred ? "deferred" : "loading") : state;
    case "request-load":
      return state === "idle" || state === "deferred" || state === "failed"
        ? "loading"
        : state;
    case "reload":
      return state === "ready" || state === "active" ? "loading" : state;
    case "ready":
      return state === "loading" ? "ready" : state;
    case "timeout":
      return state === "loading" ? "failed" : state;
    case "error":
      return state === "loading" ? "failed" : state;
    case "activate":
      return state === "ready" ? "active" : state;
    case "release":
      return state === "active" ? "ready" : state;
  }
}

export type LiveDemoFit = {
  appHeight: number;
  appWidth: number;
  scale: number;
};

/**
 * Fits the demo app into the area it is shown in. The app's CSS size is the area divided by
 * the scale, so scaling it down to the area shows the whole app. The scale never exceeds 1
 * (no upscaling), and the app is never smaller than the minimum app size.
 */
export function getLiveDemoFit(
  area: { height: number; width: number },
  minimum: { height: number; width: number } = {
    height: PRODUCT_LOOP_LIVE_DEMO.minAppHeight,
    width: PRODUCT_LOOP_LIVE_DEMO.minAppWidth,
  },
): LiveDemoFit {
  if (!(area.width > 0 && area.height > 0)) {
    return { appHeight: minimum.height, appWidth: minimum.width, scale: 1 };
  }
  const scale = Math.min(1, area.width / minimum.width, area.height / minimum.height);
  return {
    appHeight: area.height / scale,
    appWidth: area.width / scale,
    scale,
  };
}

export type LiveDemoConnection = {
  effectiveType?: string;
  saveData?: boolean;
};

/** Save-Data and 2g connections wait for an explicit button; everything else loads near the stage. */
export function shouldAutoLoadLiveDemo(
  connection: LiveDemoConnection | null | undefined,
): boolean {
  if (!connection) return true;
  if (connection.saveData) return false;
  return connection.effectiveType !== "2g" && connection.effectiveType !== "slow-2g";
}

/** The demo runs only while it is loaded, its stage is showing and the tab is visible. */
export function shouldPlayLiveDemo({
  stageVisible,
  state,
  tabVisible,
}: {
  stageVisible: boolean;
  state: LiveDemoState;
  tabVisible: boolean;
}): boolean {
  return (state === "ready" || state === "active") && stageVisible && tabVisible;
}

export function createSiteMessage(
  type: SiteToDemoMessage["type"],
): SiteToDemoMessage {
  return { source: LIVE_DEMO_SOURCE, v: LIVE_DEMO_VERSION, type } as SiteToDemoMessage;
}

/** Accepts only the `{ source: 'verevon-demo', v: 1 }` envelope with a known type. */
export function parseDemoMessage(data: unknown): DemoToSiteMessage | null {
  if (typeof data !== "object" || data === null) return null;
  const record = data as Record<string, unknown>;
  if (record.source !== LIVE_DEMO_SOURCE || record.v !== LIVE_DEMO_VERSION) return null;
  if (record.type === "ready") {
    return { source: LIVE_DEMO_SOURCE, v: LIVE_DEMO_VERSION, type: "ready" };
  }
  if (record.type === "escape") {
    return { source: LIVE_DEMO_SOURCE, v: LIVE_DEMO_VERSION, type: "escape" };
  }
  if (record.type === "error") {
    return {
      message:
        typeof record.message === "string"
          ? record.message.slice(0, LIVE_DEMO_ERROR_TEXT_LIMIT)
          : "",
      source: LIVE_DEMO_SOURCE,
      type: "error",
      v: LIVE_DEMO_VERSION,
    };
  }
  return null;
}

/**
 * A message counts only when it comes from the iframe's own window and has the envelope.
 * The sandbox gives the frame an opaque origin, so the origin is not checked.
 */
export function readDemoMessage({
  data,
  frameWindow,
  source,
}: {
  data: unknown;
  frameWindow: unknown;
  source: unknown;
}): DemoToSiteMessage | null {
  if (!frameWindow || source !== frameWindow) return null;
  return parseDemoMessage(data);
}

/**
 * Visitor-visible Norwegian copy of the live demo. Kept here so the copy is checked in
 * one place and the component stays free of literals.
 */
export const LIVE_DEMO_COPY = {
  activeAnnouncement: "Demoen er aktiv. Trykk Esc for å gå tilbake til siden.",
  badge: "Demo · fiktive data",
  closeDialog: "Lukk",
  deferred: "Last inn demoen",
  exit: "Avslutt demo",
  failed: "Demoen kunne ikke lastes akkurat nå.",
  hintActive: "Du styrer demoen. Trykk Esc for å gå videre.",
  hintReady: "Klikk i demoen for å bruke den. Rull videre når du vil.",
  loading: "Laster demoen …",
  noscript: "Demoen trenger JavaScript.",
  open: "Åpne demoen",
  readyAnnouncement: "Demoen er klar.",
  region: "Interaktiv demo av Verevon",
  releasedAnnouncement: "Demoen er lukket.",
  reset: "Start på nytt",
  resetAnnouncement: "Demoen er startet på nytt.",
  retry: "Prøv igjen",
  shieldLabel: "Klikk for å prøve dashbordet. Interaktiv demo med fiktive data.",
  shieldNote: "Ekte produkt · fiktive data · ingenting sendes",
  shieldTitle: "Klikk for å prøve",
} as const;
