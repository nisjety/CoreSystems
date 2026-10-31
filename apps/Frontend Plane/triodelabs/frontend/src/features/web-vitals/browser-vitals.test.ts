import { afterEach, describe, expect, it, vi } from "vitest";
import { installWebVitals } from "./browser-vitals";
import { setDevelopmentConsent } from "./development-consent";

function fixture() {
  type TestMetric = { name: string; value: number; entries?: unknown[] };
  const callbacks: Record<string, (value: TestMetric) => void> = {};
  const load = vi.fn(async () => ({
    onLCP: (fn: (v: TestMetric) => void) => {
      callbacks.LCP = fn;
    },
    onINP: (fn: (v: TestMetric) => void) => {
      callbacks.INP = fn;
    },
    onCLS: (fn: (v: TestMetric) => void) => {
      callbacks.CLS = fn;
    },
  }));
  const location = { href: "https://www.triodelab.no/", reload: vi.fn() };
  const history = {
    pushState: vi.fn(
      (_data: unknown, _unused: string, url?: string | URL | null) => {
        if (url) location.href = new URL(url, location.href).href;
      },
    ),
    replaceState: vi.fn(
      (_data: unknown, _unused: string, url?: string | URL | null) => {
        if (url) location.href = new URL(url, location.href).href;
      },
    ),
  };
  const win = Object.assign(new EventTarget(), {
    Cookiebot: { consent: { statistics: false } },
    navigator: { doNotTrack: "0", globalPrivacyControl: false },
    location,
    history,
    innerWidth: 1200,
    crypto: { randomUUID: () => "11111111-1111-4111-8111-111111111111" },
  });
  const send = vi.fn().mockResolvedValue(new Response(null, { status: 204 }));
  const install = () =>
    installWebVitals(
      win as unknown as Window,
      { enabled: true, development: false },
      load,
      send,
    );
  const accept = async () => {
    win.Cookiebot.consent.statistics = true;
    win.dispatchEvent(new Event("CookiebotOnAccept"));
    await Promise.resolve();
    await Promise.resolve();
  };
  return { win, load, send, callbacks, install, accept };
}
afterEach(() => {
  vi.unstubAllGlobals();
  vi.unstubAllEnvs();
});
describe("consent-gated browser Web Vitals", () => {
  it.each([
    ["production", "true"],
    ["development", "false"],
  ])(
    "cannot enable localhost through caller options under %s / flag %s",
    async (runtime, flag) => {
      vi.stubEnv("NODE_ENV", runtime);
      vi.stubEnv("NEXT_PUBLIC_WEB_VITALS_ENABLED", flag);
      const f = fixture();
      f.win.location.href = "http://localhost:3000/";
      f.win.Cookiebot.consent.statistics = true;
      installWebVitals(
        f.win as unknown as Window,
        { enabled: true, development: true },
        f.load,
        f.send,
      );
      await Promise.resolve();
      expect(f.load).not.toHaveBeenCalled();
    },
  );
  it("uses explicit local consent without Cookiebot or any third-party requests", async () => {
    vi.stubEnv("NODE_ENV", "development");
    vi.stubEnv("NEXT_PUBLIC_WEB_VITALS_ENABLED", "true");
    const f = fixture();
    f.win.location.href = "http://localhost:3000/";
    installWebVitals(
      f.win as unknown as Window,
      { enabled: true, development: true },
      f.load,
      f.send,
    );
    expect(f.load).not.toHaveBeenCalled();
    expect(setDevelopmentConsent(f.win as unknown as Window, true, true)).toBe(
      true,
    );
    await Promise.resolve();
    await Promise.resolve();
    expect(f.load).toHaveBeenCalledTimes(1);
    expect(f.win.Cookiebot.consent.statistics).toBe(false);
    f.callbacks.LCP({ name: "LCP", value: 123 });
    expect(f.send).toHaveBeenCalledTimes(1);
    expect(f.send.mock.calls[0][0]).toBe("/api/web-vitals");
    setDevelopmentConsent(f.win as unknown as Window, true, false);
    f.callbacks.CLS({ name: "CLS", value: 0.1 });
    expect(f.send).toHaveBeenCalledTimes(1);
    expect(f.win.location.reload).toHaveBeenCalledTimes(1);
    setDevelopmentConsent(f.win as unknown as Window, true, false);
    expect(f.win.location.reload).toHaveBeenCalledTimes(1);
  });
  it("does nothing when disabled and handles unavailable collection without exposing data", async () => {
    const disabled = fixture();
    installWebVitals(
      disabled.win as unknown as Window,
      { enabled: false, development: false },
      disabled.load,
      disabled.send,
    )();
    await disabled.accept();
    expect(disabled.load).not.toHaveBeenCalled();
    const f = fixture();
    const unavailable = vi.fn();
    f.win.addEventListener("triodelab:web-vitals-unavailable", unavailable);
    f.install();
    await f.accept();
    f.send.mockResolvedValueOnce(new Response(null, { status: 503 }));
    f.callbacks.CLS({ name: "CLS", value: 0 });
    await Promise.resolve();
    f.send.mockRejectedValueOnce(new Error("unavailable"));
    f.callbacks.INP({ name: "INP", value: 100 });
    await Promise.resolve();
    await Promise.resolve();
    expect(unavailable).toHaveBeenCalledTimes(2);
    f.callbacks.LCP({ name: "LCP", value: -1 });
    expect(f.send).toHaveBeenCalledTimes(2);
  });
  it("stops after a failed observer import", async () => {
    const f = fixture();
    const load = vi.fn().mockRejectedValue(new Error("unavailable"));
    installWebVitals(
      f.win as unknown as Window,
      { enabled: true, development: false },
      load,
      f.send,
    );
    await f.accept();
    await Promise.resolve();
    await f.accept();
    expect(load).toHaveBeenCalledTimes(1);
  });
  it("resets per-metric sequence on BFCache restoration", async () => {
    const f = fixture();
    f.install();
    await f.accept();
    f.callbacks.CLS({ name: "CLS", value: 0 });
    f.callbacks.CLS({ name: "CLS", value: 0.1 });
    const event = new Event("pageshow");
    Object.defineProperty(event, "persisted", { value: true });
    f.win.dispatchEvent(event);
    f.callbacks.CLS({ name: "CLS", value: 0 });
    expect(
      f.send.mock.calls.map(([, init]) => JSON.parse(init.body).sequence),
    ).toEqual([1, 2, 1]);
  });
  it("keeps consent listeners until the last attachment is removed", async () => {
    const f = fixture();
    const first = f.install();
    const second = f.install();
    first();
    await f.accept();
    expect(f.load).toHaveBeenCalledTimes(1);
    f.win.Cookiebot.consent.statistics = false;
    f.win.dispatchEvent(new Event("CookiebotOnDecline"));
    expect(f.win.location.reload).toHaveBeenCalledTimes(1);
    second();
  });
  it("permanently stops document metrics after SPA navigation, even when returning", async () => {
    const f = fixture();
    f.install();
    await f.accept();
    f.win.history.pushState(null, "", "/kontakt");
    f.win.history.replaceState(null, "", "/");
    f.callbacks.LCP({ name: "LCP", value: 120 });
    expect(f.send).not.toHaveBeenCalled();
  });
  it("does not report after cleanup and reattaches without duplicate observers", async () => {
    const f = fixture();
    const cleanup = f.install();
    await f.accept();
    cleanup();
    f.callbacks.LCP({ name: "LCP", value: 90 });
    expect(f.send).not.toHaveBeenCalled();
    f.install();
    f.callbacks.LCP({ name: "LCP", value: 120 });
    expect(f.send).toHaveBeenCalledTimes(1);
    expect(f.load).toHaveBeenCalledTimes(1);
  });
  it("does not register callbacks from a late import while detached", async () => {
    const f = fixture();
    let finish: ((v: Awaited<ReturnType<typeof f.load>>) => void) | undefined;
    const pending = () =>
      new Promise<Awaited<ReturnType<typeof f.load>>>((resolve) => {
        finish = resolve;
      });
    const cleanup = installWebVitals(
      f.win as unknown as Window,
      { enabled: true, development: false },
      pending,
      f.send,
    );
    await f.accept();
    cleanup();
    const library = await f.load();
    finish?.(library);
    await Promise.resolve();
    expect(f.callbacks.LCP).toBeUndefined();
    installWebVitals(
      f.win as unknown as Window,
      { enabled: true, development: false },
      pending,
      f.send,
    );
    expect(f.callbacks.LCP).toBeDefined();
  });
  it("does not import observers or send before Statistics consent and initializes once", async () => {
    const f = fixture();
    const stop = f.install();
    expect(f.load).not.toHaveBeenCalled();
    await f.accept();
    stop();
    f.install();
    await f.accept();
    expect(f.load).toHaveBeenCalledTimes(1);
    f.callbacks.LCP({
      name: "LCP",
      value: 1221,
      entries: [{ element: "private" }],
    });
    expect(f.send).toHaveBeenCalledTimes(1);
    const [url, init] = f.send.mock.calls[0];
    expect(url).toBe("/api/web-vitals");
    expect(init.credentials).toBe("omit");
    expect(init.referrerPolicy).toBe("no-referrer");
    expect(JSON.parse(init.body)).toEqual({
      sampleId: "11111111-1111-4111-8111-111111111111",
      sequence: 1,
      metric: "LCP",
      value: 1221,
      viewport: "wide",
      path: "/",
    });
    expect(init.body).not.toContain("entries");
  });
  it("rechecks consent, DNT/GPC, current URL and sends no fabricated missing metrics", async () => {
    const f = fixture();
    f.install();
    await f.accept();
    expect(f.send).not.toHaveBeenCalled();
    f.callbacks.CLS({ name: "CLS", value: 0 });
    expect(f.send).toHaveBeenCalledTimes(1);
    f.win.navigator.globalPrivacyControl = true;
    f.callbacks.INP({ name: "INP", value: 100 });
    expect(f.send).toHaveBeenCalledTimes(1);
    const g = fixture();
    g.install();
    await g.accept();
    g.win.location.href += "?private=x";
    g.callbacks.LCP({ name: "LCP", value: 30 });
    expect(g.send).not.toHaveBeenCalled();
    const h = fixture();
    h.install();
    await h.accept();
    h.win.Cookiebot.consent.statistics = false;
    h.win.dispatchEvent(new Event("CookiebotOnDecline"));
    h.callbacks.LCP({ name: "LCP", value: 30 });
    expect(h.send).not.toHaveBeenCalled();
  });
});
