"use client";

import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useReducer,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import { Maximize2, Minimize2 } from "lucide-react";
import { productLoopPrefersReducedMotion } from "./ProductLoopProductDemos";
import {
  LIVE_DEMO_BAR_HEIGHT,
  LIVE_DEMO_COPY,
  LIVE_DEMO_READY_TIMEOUT_MS,
  LIVE_DEMO_SANDBOX,
  createSiteMessage,
  getLiveDemoFit,
  readDemoMessage,
  reduceLiveDemo,
  shouldAutoLoadLiveDemo,
  shouldPlayLiveDemo,
  type LiveDemoConnection,
  type LiveDemoState,
  type ProductLoopLiveDemo,
  type SiteToDemoMessage,
} from "./product-loop-live-demo";
import {
  PRODUCT_LOOP_SETTLE_EVENT,
  getProductLoopContentFrame,
} from "./product-loop-progress";

// The live demo on the stage (desktop, inside the film frame) and in the mobile dialog. Both are
// the same iframe: the stage keeps it inside the pinned loop, the dialog shows it full screen.

type DemoVariant = "stage" | "dialog";
type ReleaseReason = "dialog" | "escape" | "exit" | "focus" | "pointer" | "stage";

const isLoaded = (state: LiveDemoState) => state === "ready" || state === "active";
const isFrameMounted = (state: LiveDemoState) => state === "loading" || isLoaded(state);
const noop = () => {};

// The site's layout shim rewrites reduced-motion media queries, so the native preference is read
// from a custom property that the motion-reduce variant sets on the demo's own root element.
function prefersReducedMotion(element: Element | null) {
  if (
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  ) {
    return true;
  }
  return element !== null && productLoopPrefersReducedMotion(element);
}

type UseLiveDemoOptions = {
  dialogOpen: boolean;
  frameRef: RefObject<HTMLIFrameElement | null>;
  onDialogEscape: () => void;
  scopeRef: RefObject<HTMLElement | null>;
  shieldRef: RefObject<HTMLButtonElement | null>;
  variant: DemoVariant;
};

// The whole lifecycle of one demo frame: loading and its timeout, the shield, activation and
// release, reset, pause and resume, and the message checks. Both variants share it. The caller
// owns the refs, so the render only passes them on.
function useLiveDemo({
  dialogOpen,
  frameRef,
  onDialogEscape,
  scopeRef,
  shieldRef,
  variant,
}: UseLiveDemoOptions) {
  const [state, dispatch] = useReducer(reduceLiveDemo, "idle");
  const [announcement, setAnnouncement] = useState("");
  const [stageVisible, setStageVisible] = useState(false);
  const [tabVisible, setTabVisible] = useState(true);
  const stateRef = useRef<LiveDemoState>("idle");
  const escapeRef = useRef(onDialogEscape);
  const playSentRef = useRef<boolean | null>(null);
  const announceResetRef = useRef(false);
  const focusShieldRef = useRef(false);
  const visibleRef = useRef(false);
  const leftRef = useRef(false);
  const visible = variant === "dialog" ? dialogOpen : stageVisible;

  useLayoutEffect(() => {
    stateRef.current = state;
    escapeRef.current = onDialogEscape;
  });

  const post = useCallback(
    (type: SiteToDemoMessage["type"]) => {
      frameRef.current?.contentWindow?.postMessage(createSiteMessage(type), "*");
    },
    [frameRef],
  );

  const release = useCallback(
    (reason: ReleaseReason) => {
      if (stateRef.current !== "active") return;
      dispatch({ type: "release" });
      post("deactivate");
      // Keyboard exits return focus to the shield; pointer, focus and stage exits leave focus alone.
      if (reason === "escape" || reason === "exit") focusShieldRef.current = true;
      setAnnouncement(LIVE_DEMO_COPY.releasedAnnouncement);
    },
    [post],
  );

  const activate = useCallback(
    (moveFocus: boolean) => {
      if (stateRef.current !== "ready") return;
      dispatch({ type: "activate" });
      post("activate");
      setAnnouncement(LIVE_DEMO_COPY.activeAnnouncement);
      if (variant !== "stage") return;
      const reducedMotion = prefersReducedMotion(scopeRef.current);
      window.dispatchEvent(
        new CustomEvent(PRODUCT_LOOP_SETTLE_EVENT, { detail: { reducedMotion } }),
      );
      if (moveFocus) frameRef.current?.focus({ preventScroll: true });
    },
    [frameRef, post, scopeRef, variant],
  );

  // The demo reloads itself on reset. The poster covers the frame until its ready message comes
  // back, and a reload that never answers fails on the load timeout, as a first load does.
  const reset = useCallback(
    (announce: boolean) => {
      if (!isLoaded(stateRef.current)) return;
      post("reset");
      announceResetRef.current = announce;
      // The reset button leaves the page while the demo reloads, so focus goes to the shield when it returns.
      if (announce) focusShieldRef.current = true;
      // Mirror the reducer at once, so the effects of this same pass do not act on a loaded demo.
      stateRef.current = reduceLiveDemo(stateRef.current, { type: "reload" });
      dispatch({ type: "reload" });
    },
    [post],
  );

  const requestLoad = useCallback(() => dispatch({ type: "request-load" }), []);

  // Messages count only from the frame's own window and with the envelope (see readDemoMessage).
  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      const message = readDemoMessage({
        data: event.data,
        frameWindow: frameRef.current?.contentWindow ?? null,
        source: event.source,
      });
      if (!message) return;
      if (message.type === "escape") {
        if (variant === "dialog") escapeRef.current();
        else release("escape");
        return;
      }
      if (message.type === "error") {
        dispatch({ type: "error" });
        return;
      }
      if (stateRef.current === "loading") setAnnouncement(LIVE_DEMO_COPY.readyAnnouncement);
      if (announceResetRef.current) {
        announceResetRef.current = false;
        setAnnouncement(LIVE_DEMO_COPY.resetAnnouncement);
      }
      dispatch({ type: "ready" });
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [frameRef, release, variant]);

  // Load when the stage nears the viewport, after the browser is idle. Save-Data and 2g wait for a button.
  useEffect(() => {
    if (variant !== "stage") return;
    const scope = scopeRef.current;
    if (!scope) return;
    let cancelIdle: (() => void) | null = null;
    const near = () => {
      const connection = (navigator as Navigator & { connection?: LiveDemoConnection })
        .connection;
      dispatch({ type: "near", deferred: !shouldAutoLoadLiveDemo(connection) });
    };
    const whenIdle = () => {
      if (typeof window.requestIdleCallback === "function") {
        const handle = window.requestIdleCallback(near, { timeout: 1000 });
        cancelIdle = () => window.cancelIdleCallback(handle);
      } else {
        const handle = window.setTimeout(near, 200);
        cancelIdle = () => window.clearTimeout(handle);
      }
    };
    if (typeof IntersectionObserver === "undefined") {
      whenIdle();
      return () => cancelIdle?.();
    }
    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return;
        observer.disconnect();
        whenIdle();
      },
      { rootMargin: "50%" },
    );
    observer.observe(scope);
    return () => {
      observer.disconnect();
      cancelIdle?.();
    };
  }, [scopeRef, variant]);

  // The stage layer tells the demo when it is showing and when the visitor has left its settled window.
  useEffect(() => {
    if (variant !== "stage") return;
    const layer = scopeRef.current?.closest<HTMLElement>("[data-product-loop-layer]");
    if (!layer) {
      setStageVisible(true);
      return;
    }
    const update = () => {
      const shown = layer.getAttribute("aria-hidden") === "false";
      setStageVisible(shown);
      if (!shown || layer.getAttribute("data-product-loop-settled") === "false") {
        release("stage");
      }
    };
    update();
    const observer = new MutationObserver(update);
    observer.observe(layer, {
      attributeFilter: ["aria-hidden", "data-product-loop-settled"],
      attributes: true,
    });
    return () => observer.disconnect();
  }, [release, scopeRef, variant]);

  useEffect(() => {
    if (variant === "dialog" && !dialogOpen) release("dialog");
  }, [dialogOpen, release, variant]);

  // Coming back to the stage (or reopening the dialog) after leaving it starts the demo afresh.
  useEffect(() => {
    if (!visible) {
      if (visibleRef.current) leftRef.current = true;
      visibleRef.current = false;
      return;
    }
    visibleRef.current = true;
    if (leftRef.current) {
      leftRef.current = false;
      reset(false);
    }
  }, [reset, visible]);

  // The dialog is an explicit choice to use the demo, so it activates as soon as the frame is ready.
  useEffect(() => {
    if (variant === "dialog" && dialogOpen && state === "ready") activate(false);
  }, [activate, dialogOpen, state, variant]);

  // Pause and resume follow the stage, the dialog and the tab. Messages go out only on a change.
  useEffect(() => {
    if (!isLoaded(state)) {
      playSentRef.current = null;
      return;
    }
    const play = shouldPlayLiveDemo({ stageVisible: visible, state, tabVisible });
    if (playSentRef.current === play) return;
    playSentRef.current = play;
    post(play ? "resume" : "pause");
  }, [post, state, tabVisible, visible]);

  useEffect(() => {
    const update = () => setTabVisible(!document.hidden);
    update();
    document.addEventListener("visibilitychange", update);
    return () => document.removeEventListener("visibilitychange", update);
  }, []);

  // While the demo is active, a pointer or focus outside the stage, or Escape, hands the page back.
  useEffect(() => {
    if (variant !== "stage" || state !== "active") return;
    const scope = scopeRef.current;
    if (!scope) return;
    const inside = (target: EventTarget | null) =>
      target instanceof Node && scope.contains(target);
    const onPointerDown = (event: PointerEvent) => {
      if (!inside(event.target)) release("pointer");
    };
    const onFocusIn = (event: FocusEvent) => {
      if (!inside(event.target)) release("focus");
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") release("escape");
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("focusin", onFocusIn, true);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("focusin", onFocusIn, true);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [release, scopeRef, state, variant]);

  useEffect(() => {
    if (state === "ready" && focusShieldRef.current) {
      focusShieldRef.current = false;
      shieldRef.current?.focus({ preventScroll: true });
    }
  }, [shieldRef, state]);

  useEffect(() => {
    if (state !== "loading") return;
    const timer = window.setTimeout(
      () => dispatch({ type: "timeout" }),
      LIVE_DEMO_READY_TIMEOUT_MS,
    );
    return () => window.clearTimeout(timer);
  }, [state]);

  return {
    activate,
    announcement,
    release,
    requestLoad,
    reset,
    state,
  };
}

type LiveDemo = ReturnType<typeof useLiveDemo>;

type FullscreenControl = {
  isFullscreen: boolean;
  isSupported: boolean;
  toggle: () => void;
};

// The browser owns full screen and restores it with its native Escape behaviour. The component
// only mirrors that state for the toolbar label and never tries to emulate full screen in CSS.
function useFullscreenControl(
  targetRef: RefObject<HTMLElement | null>,
): FullscreenControl {
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [isSupported, setIsSupported] = useState(false);

  useEffect(() => {
    const target = targetRef.current;
    if (!target) return;

    const update = () => {
      setIsFullscreen(document.fullscreenElement === target);
    };
    const supported =
      typeof target.requestFullscreen === "function" &&
      typeof document.exitFullscreen === "function";

    setIsSupported(supported);
    update();
    document.addEventListener("fullscreenchange", update);
    return () => document.removeEventListener("fullscreenchange", update);
  }, [targetRef]);

  const toggle = useCallback(() => {
    const target = targetRef.current;
    if (
      !target ||
      typeof target.requestFullscreen !== "function" ||
      typeof document.exitFullscreen !== "function"
    ) {
      setIsSupported(false);
      return;
    }

    const action =
      document.fullscreenElement === target
        ? document.exitFullscreen()
        : target.requestFullscreen();

    void action.catch(() => setIsSupported(false));
  }, [targetRef]);

  return { isFullscreen, isSupported, toggle };
}

// Sizes the demo app from the settled film frame and scales it to the frame as it morphs.
// The app's CSS size changes only with the viewport, so the frame's morph never reflows the app.
function useLiveDemoFit({
  appRef,
  areaRef,
  config,
  scopeRef,
}: {
  appRef: RefObject<HTMLDivElement | null>;
  areaRef: RefObject<HTMLDivElement | null>;
  config: ProductLoopLiveDemo;
  scopeRef: RefObject<HTMLElement | null>;
}) {
  const minimumWidth = config.minAppWidth;
  const minimumHeight = config.minAppHeight;
  useLayoutEffect(() => {
    const area = areaRef.current;
    const app = appRef.current;
    if (!area || !app) return;
    const viewport =
      scopeRef.current?.closest<HTMLElement>("[data-product-loop-viewport]") ?? null;
    const minimum = { height: minimumHeight, width: minimumWidth };

    const applyScale = () => {
      const width = Number.parseFloat(app.style.width);
      const height = Number.parseFloat(app.style.height);
      if (!(width > 0 && height > 0)) return;
      const rect = area.getBoundingClientRect();
      const scale = Math.min(rect.width / width, rect.height / height);
      app.style.transform = `scale(${Math.max(0, scale)})`;
    };

    const applySize = () => {
      const frame = viewport
        ? getProductLoopContentFrame(viewport.getBoundingClientRect())
        : null;
      const size = frame
        ? { height: frame.height - LIVE_DEMO_BAR_HEIGHT, width: frame.width }
        : area.getBoundingClientRect();
      const fit = getLiveDemoFit(size, minimum);
      app.style.width = `${fit.appWidth}px`;
      app.style.height = `${fit.appHeight}px`;
      applyScale();
    };

    applySize();
    if (typeof ResizeObserver === "undefined") return;
    const sizeObserver = new ResizeObserver(applySize);
    if (viewport) sizeObserver.observe(viewport);
    const scaleObserver = new ResizeObserver(applyScale);
    scaleObserver.observe(area);
    return () => {
      sizeObserver.disconnect();
      scaleObserver.disconnect();
    };
  }, [appRef, areaRef, minimumHeight, minimumWidth, scopeRef]);
}

function LiveDemoButton({
  children,
  onClick,
}: {
  children: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      className="rounded-full border border-verevon-j-text/15 bg-background px-3 py-1 text-[13px] text-verevon-j-text transition-colors hover:border-verevon-j-text/35 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-verevon-j-text"
      onClick={onClick}
      type="button"
    >
      {children}
    </button>
  );
}

function LiveDemoFullscreenButton({
  fullscreen,
}: {
  fullscreen: FullscreenControl;
}) {
  const label = fullscreen.isFullscreen
    ? "Avslutt fullskjerm"
    : "Vis dashboard i fullskjerm";

  return (
    <button
      aria-label={label}
      aria-pressed={fullscreen.isFullscreen}
      className="grid size-7 place-items-center rounded-full border border-verevon-j-text/15 bg-background text-verevon-j-text transition-colors hover:border-verevon-j-text/35 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-verevon-j-text"
      onClick={fullscreen.toggle}
      title={label}
      type="button"
    >
      {fullscreen.isFullscreen ? (
        <Minimize2 aria-hidden="true" size={14} strokeWidth={1.6} />
      ) : (
        <Maximize2 aria-hidden="true" size={14} strokeWidth={1.6} />
      )}
    </button>
  );
}

// The demo area: the frame (below the poster), the poster until the frame is ready, the shield
// before activation on the stage, and the loading, deferred and failure messages.
function LiveDemoArea({
  appRef,
  areaRef,
  bottom,
  config,
  demo,
  frameRef,
  posterSrc,
  shieldRef,
  variant,
}: {
  appRef?: RefObject<HTMLDivElement | null>;
  areaRef?: RefObject<HTMLDivElement | null>;
  bottom: number;
  config: ProductLoopLiveDemo;
  demo: LiveDemo;
  frameRef: RefObject<HTMLIFrameElement | null>;
  posterSrc: string;
  shieldRef: RefObject<HTMLButtonElement | null>;
  variant: DemoVariant;
}) {
  const [posterMissing, setPosterMissing] = useState(false);
  const { state } = demo;
  const shown = isLoaded(state);
  const waiting = state === "deferred" || state === "loading" || state === "failed";

  return (
    <div
      className="absolute inset-x-0 top-0 overflow-hidden bg-[radial-gradient(circle_at_78%_28%,rgba(238,122,80,0.08),transparent_27%),linear-gradient(180deg,#f8f8f7,#efeee9)]"
      ref={areaRef}
      style={{ bottom }}
    >
      <div className="absolute left-0 top-0 h-full w-full origin-top-left" ref={appRef}>
        {isFrameMounted(state) && (
          <iframe
            aria-hidden={state === "active" ? undefined : true}
            className={`block h-full w-full border-0 bg-[#f8f8f7] transition-opacity duration-200 motion-reduce:transition-none ${
              shown ? "opacity-100" : "opacity-0"
            }`}
            ref={frameRef}
            referrerPolicy="same-origin"
            sandbox={LIVE_DEMO_SANDBOX}
            src={config.src}
            tabIndex={state === "active" ? undefined : -1}
            title={config.frameTitle}
          />
        )}
      </div>

      {/* The poster covers the frame until it is ready, so the page keeps the wheel until then. */}
      <div
        className={`absolute inset-0 overflow-hidden bg-[radial-gradient(circle_at_78%_28%,rgba(238,122,80,0.08),transparent_27%),linear-gradient(180deg,#f8f8f7,#efeee9)] transition-opacity duration-200 motion-reduce:transition-none ${
          shown ? "pointer-events-none opacity-0" : "opacity-100"
        }`}
      >
        {!posterMissing && (
          // The poster is a static WebP in the public folder; the CSS background is the fallback.
          // eslint-disable-next-line @next/next/no-img-element
          <img
            alt={config.posterAlt}
            className="h-full w-full object-cover object-left-top"
            decoding="async"
            loading="lazy"
            onError={() => setPosterMissing(true)}
            src={posterSrc}
          />
        )}
      </div>

      {variant === "stage" && state === "ready" && (
        <button
          aria-label={LIVE_DEMO_COPY.shieldLabel}
          className="absolute inset-0 flex cursor-pointer items-center justify-center bg-transparent focus-visible:outline-2 focus-visible:-outline-offset-4 focus-visible:outline-verevon-j-text"
          onClick={() => demo.activate(true)}
          ref={shieldRef}
          type="button"
        >
          <span className="pointer-events-none rounded-full border border-verevon-j-text/12 bg-background/92 px-5 py-2.5 text-center shadow-[0_10px_30px_rgba(23,23,23,0.12)]">
            <span className="block font-manrope text-[15px] text-verevon-j-text">
              {LIVE_DEMO_COPY.shieldTitle}
            </span>
            <span className="mt-0.5 block text-[12px] text-verevon-text-muted">
              {LIVE_DEMO_COPY.shieldNote}
            </span>
          </span>
        </button>
      )}

      {waiting && (
        <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center gap-4 px-6 text-center">
          {state === "loading" && (
            <p className="rounded-full bg-background/90 px-4 py-2 text-[14px] text-verevon-j-text">
              {LIVE_DEMO_COPY.loading}
            </p>
          )}
          {state === "deferred" && (
            <div className="pointer-events-auto">
              <LiveDemoButton onClick={() => demo.requestLoad()}>{LIVE_DEMO_COPY.deferred}</LiveDemoButton>
            </div>
          )}
          {state === "failed" && (
            <>
              <p className="rounded-full bg-background/90 px-4 py-2 text-[14px] text-verevon-j-text">
                {LIVE_DEMO_COPY.failed}
              </p>
              <div className="pointer-events-auto">
                <LiveDemoButton onClick={() => demo.requestLoad()}>{LIVE_DEMO_COPY.retry}</LiveDemoButton>
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}

// The stage on desktop: the frame of stage 04 in the pinned loop, with a 44px bar at its bottom edge.
export function LoopLiveDemo({ live }: { live: ProductLoopLiveDemo }) {
  const scopeRef = useRef<HTMLElement>(null);
  const areaRef = useRef<HTMLDivElement>(null);
  const appRef = useRef<HTMLDivElement>(null);
  const frameRef = useRef<HTMLIFrameElement>(null);
  const shieldRef = useRef<HTMLButtonElement>(null);
  const demo = useLiveDemo({
    dialogOpen: false,
    frameRef,
    onDialogEscape: noop,
    scopeRef,
    shieldRef,
    variant: "stage",
  });
  const fullscreen = useFullscreenControl(scopeRef);
  useLiveDemoFit({ appRef, areaRef, config: live, scopeRef });
  const { state } = demo;
  const hint =
    state === "active"
      ? LIVE_DEMO_COPY.hintActive
      : state === "ready"
        ? LIVE_DEMO_COPY.hintReady
        : "";

  return (
    <section
      aria-label={LIVE_DEMO_COPY.region}
      className="absolute inset-0 overflow-hidden bg-[#f8f8f7] text-verevon-j-text motion-reduce:[--product-loop-motion:reduce]"
		data-fullscreen={fullscreen.isFullscreen}
      data-product-loop-live=""
      data-state={state}
      ref={scopeRef}
    >
      <LiveDemoArea
        appRef={appRef}
        areaRef={areaRef}
        bottom={LIVE_DEMO_BAR_HEIGHT}
        config={live}
        demo={demo}
        frameRef={frameRef}
        posterSrc={live.poster}
        shieldRef={shieldRef}
        variant="stage"
      />
      <div
        className="absolute inset-x-0 bottom-0 flex items-center gap-3 border-t border-verevon-j-text/10 bg-[#f8f8f7] px-4 text-[13px] text-verevon-text-muted"
        style={{ height: LIVE_DEMO_BAR_HEIGHT }}
      >
        <span className="shrink-0 rounded-full border border-verevon-j-text/12 px-2.5 py-0.5 text-[12px] text-verevon-j-text">
          {LIVE_DEMO_COPY.badge}
        </span>
        <p className="min-w-0 truncate">{hint}</p>
        <div className="ml-auto flex shrink-0 items-center gap-2">
          {isLoaded(state) && fullscreen.isSupported && (
            <LiveDemoFullscreenButton fullscreen={fullscreen} />
          )}
          {isLoaded(state) && (
            <LiveDemoButton onClick={() => demo.reset(true)}>
              {LIVE_DEMO_COPY.reset}
            </LiveDemoButton>
          )}
          {state === "active" && (
            <LiveDemoButton onClick={() => demo.release("exit")}>
              {LIVE_DEMO_COPY.exit}
            </LiveDemoButton>
          )}
        </div>
      </div>
      <p className="sr-only" role="status">
        {demo.announcement}
      </p>
    </section>
  );
}

// Below 900px: a poster card. «Åpne demoen» opens the same frame full screen in a modal dialog.
// The frame is mounted only after that click and is kept mounted when the dialog closes.
export function LiveDemoMobileCard({ live }: { live: ProductLoopLiveDemo }) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const frameRef = useRef<HTMLIFrameElement>(null);
  const shieldRef = useRef<HTMLButtonElement>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [posterMissing, setPosterMissing] = useState(false);
  const demo = useLiveDemo({
    dialogOpen,
    frameRef,
    onDialogEscape: () => dialogRef.current?.close(),
    scopeRef: dialogRef,
    shieldRef,
    variant: "dialog",
  });

  const openDemo = () => {
    const dialog = dialogRef.current;
    // Without <dialog> support the poster card stays as it is and the demo is not offered.
    if (!dialog || typeof dialog.showModal !== "function" || dialog.open) return;
    dialog.showModal();
    setDialogOpen(true);
    demo.requestLoad();
  };

  const closeDemo = () => dialogRef.current?.close();

  const onDialogClose = () => {
    setDialogOpen(false);
    triggerRef.current?.focus({ preventScroll: true });
  };

  // The page does not scroll behind the open dialog.
  useEffect(() => {
    if (!dialogOpen) return;
    const root = document.documentElement;
    const previous = root.style.overflow;
    root.style.overflow = "hidden";
    return () => {
      root.style.overflow = previous;
    };
  }, [dialogOpen]);

  // Keeps Tab and focus inside the open dialog.
  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialogOpen || !dialog) return;
    const focusable = () =>
      Array.from(
        dialog.querySelectorAll<HTMLElement>(
          "button:not([disabled]), iframe:not([tabindex='-1'])",
        ),
      );
    const onFocusIn = (event: FocusEvent) => {
      // A closing dialog moves focus back to its trigger before this effect is torn down.
      if (!dialog.hasAttribute("open")) return;
      if (event.target instanceof Node && dialog.contains(event.target)) return;
      focusable()[0]?.focus({ preventScroll: true });
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Tab") return;
      const items = focusable();
      const first = items[0];
      const last = items[items.length - 1];
      if (!first || !last) return;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus({ preventScroll: true });
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus({ preventScroll: true });
      }
    };
    document.addEventListener("focusin", onFocusIn, true);
    dialog.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("focusin", onFocusIn, true);
      dialog.removeEventListener("keydown", onKeyDown);
    };
  }, [dialogOpen]);

  return (
    <div className="grid gap-3">
      <div className="relative aspect-[4/5] overflow-hidden rounded-[20px] bg-[radial-gradient(circle_at_78%_28%,rgba(238,122,80,0.08),transparent_27%),linear-gradient(180deg,#f8f8f7,#efeee9)]">
        {!posterMissing && (
          // eslint-disable-next-line @next/next/no-img-element
          <img
            alt={live.posterAlt}
            className="absolute inset-0 h-full w-full object-cover object-left-top"
            decoding="async"
            loading="lazy"
            onError={() => setPosterMissing(true)}
            src={live.posterMobile}
          />
        )}
        <span className="absolute left-4 top-4 rounded-full border border-verevon-j-text/12 bg-background/90 px-3 py-1 text-[13px] text-verevon-j-text">
          {LIVE_DEMO_COPY.badge}
        </span>
        <button
          className="absolute bottom-4 left-4 right-4 rounded-full bg-verevon-j-text px-5 py-3 text-[15px] text-background"
          onClick={openDemo}
          ref={triggerRef}
          type="button"
        >
          {LIVE_DEMO_COPY.open}
        </button>
      </div>
      <noscript>
        <p className="text-[14px] text-verevon-text-muted">{LIVE_DEMO_COPY.noscript}</p>
      </noscript>
      <dialog
        aria-label={LIVE_DEMO_COPY.region}
        // No display utility here: it would override the browser's rule that hides a closed dialog.
        className="fixed inset-0 m-0 h-dvh w-full max-h-none max-w-none border-0 bg-background p-0 text-verevon-j-text backdrop:bg-background"
        data-state={demo.state}
        onClose={onDialogClose}
        ref={dialogRef}
      >
        <div className="flex h-full flex-col">
        <div className="flex h-[52px] shrink-0 items-center gap-3 border-b border-verevon-j-text/10 px-4">
          <span className="text-[13px] text-verevon-j-text">{LIVE_DEMO_COPY.badge}</span>
          <div className="ml-auto flex items-center gap-2">
            {isLoaded(demo.state) && (
              <LiveDemoButton onClick={() => demo.reset(true)}>
                {LIVE_DEMO_COPY.reset}
              </LiveDemoButton>
            )}
            <LiveDemoButton onClick={closeDemo}>{LIVE_DEMO_COPY.closeDialog}</LiveDemoButton>
          </div>
        </div>
        <div className="relative min-h-0 flex-1">
          <LiveDemoArea
            bottom={0}
            config={live}
            demo={demo}
            frameRef={frameRef}
            posterSrc={live.posterMobile}
            shieldRef={shieldRef}
            variant="dialog"
          />
        </div>
        <p className="sr-only" role="status">
          {demo.announcement}
        </p>
        </div>
      </dialog>
    </div>
  );
}
