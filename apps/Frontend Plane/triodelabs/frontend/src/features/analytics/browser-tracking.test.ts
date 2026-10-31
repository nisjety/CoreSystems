import { afterEach, describe, expect, it, vi } from "vitest";
import { installBrowserTracking } from "./browser-tracking";

class FakeElement extends EventTarget {
  id = "";
  src = "";
  async = false;
  referrerPolicy = "";
  href = "";
  target = "";
  sensitive = false;
  consentControl = false;
  anchor = false;
  attributes = new Map<string, string>();
  setAttribute(name: string, value: string) {
    this.attributes.set(name, value);
  }
  hasAttribute(name: string) {
    return this.attributes.has(name);
  }
  closest(selector: string) {
    if (selector === "a[href]") return this.anchor ? this : null;
    if (selector === "#CybotCookiebotDialog, #CookiebotWidget")
      return this.consentControl ? this : null;
    return this.sensitive ? this : null;
  }
}

function fixture() {
  vi.stubGlobal("Element", FakeElement);
  const scripts: FakeElement[] = [];
  const doc = Object.assign(new EventTarget(), {
    referrer: "",
    cookie: "_ga=test; _clck=test; CookieConsent=keep; session=keep",
    getElementById: (id: string) => scripts.find((script) => script.id === id),
    createElement: () => new FakeElement(),
    head: { appendChild: (script: FakeElement) => scripts.push(script) },
  });
  const win = Object.assign(new EventTarget(), {
    Cookiebot: { consent: { statistics: false } },
    navigator: { doNotTrack: "0" },
    location: {
      href: "https://www.triodelab.no/",
      hostname: "www.triodelab.no",
      origin: "https://www.triodelab.no",
      reload: vi.fn(),
      assign: vi.fn(),
    },
    history: { pushState: vi.fn(), replaceState: vi.fn() },
  });
  const config = {
    measurementId: "G-TEST123456",
    clarityProjectId: "test123456",
  };
  const install = () =>
    installBrowserTracking(
      win as unknown as Window,
      doc as unknown as Document,
      config,
    );
  const accept = () => {
    win.Cookiebot.consent.statistics = true;
    win.dispatchEvent(new Event("CookiebotOnAccept"));
  };
  const tracking = win as typeof win & {
    dataLayer: IArguments[];
    clarity: { q: unknown[][] };
    "ga-disable-G-TEST123456": boolean;
  };
  return { win, doc, scripts, config, install, accept, tracking };
}

afterEach(() => vi.unstubAllGlobals());

describe("browser tracking integration, without external network", () => {
  it("never inserts external scripts before explicit consent, including declined and incomplete CMP state", () => {
    const { install, win, scripts } = fixture();
    install();
    win.dispatchEvent(new Event("CookiebotOnConsentReady"));
    win.dispatchEvent(new Event("CookiebotOnDecline"));
    expect(scripts).toHaveLength(0);
  });

  it("loads both public-ID scripts exactly once on consent and marks them Statistics-only", () => {
    const { install, accept, scripts, tracking } = fixture();
    const cleanup = install();
    accept();
    cleanup();
    install(); // React StrictMode cleanup/setup must not duplicate scripts or page views.
    accept();
    expect(scripts).toHaveLength(2);
    expect(
      scripts.every(
        (script) =>
          script.async &&
          script.referrerPolicy === "no-referrer" &&
          script.attributes.get("data-cookieconsent") === "statistics",
      ),
    ).toBe(true);
    expect(tracking.clarity.q).toEqual([
      ["consentv2", { analytics_Storage: "granted", ad_Storage: "denied" }],
    ]);
    expect(
      tracking.dataLayer
        .map((args) => Array.from(args))
        .filter(([command]) => command === "config"),
    ).toHaveLength(1);
    expect(tracking.dataLayer.some((args) => args[0] === "event")).toBe(false);
  });

  it("revokes consent, disables GA, stops Clarity and recreates the document", () => {
    const { install, accept, win, tracking } = fixture();
    install();
    accept();
    win.Cookiebot.consent.statistics = false;
    win.dispatchEvent(new Event("CookiebotOnDecline"));
    expect(tracking["ga-disable-G-TEST123456"]).toBe(true);
    expect(tracking.clarity.q.at(-1)).toEqual(["stop"]);
    expect(win.location.reload).toHaveBeenCalledTimes(1);
  });

  it("stops before programmatic private-route navigation and replaces it with document navigation", () => {
    const { install, accept, win, tracking } = fixture();
    const original = win.history.pushState;
    install();
    accept();
    (win.history as unknown as History).pushState({}, "", "/kontakt");
    expect(original).not.toHaveBeenCalled();
    expect(tracking.clarity.q.at(-1)).toEqual(["stop"]);
    expect(win.location.assign).toHaveBeenCalledWith(
      "https://www.triodelab.no/kontakt",
    );
  });

  it("leaves history/navigation untouched while Statistics consent is absent", () => {
    const { install, win } = fixture();
    const original = win.history.pushState;
    install();
    (win.history as unknown as History).pushState({}, "", "/kontakt");
    win.dispatchEvent(new Event("popstate"));
    expect(original).toHaveBeenCalledWith({}, "", "/kontakt");
    expect(win.location.reload).not.toHaveBeenCalled();
  });

  it("stops on back/forward navigation and restores its wrappers/listeners on cleanup", () => {
    const { install, accept, win } = fixture();
    const originalPush = win.history.pushState;
    const originalReplace = win.history.replaceState;
    const cleanup = install();
    accept();
    win.dispatchEvent(new Event("popstate"));
    expect(win.location.reload).toHaveBeenCalledTimes(1);
    cleanup();
    expect(win.history.pushState).toBe(originalPush);
    expect(win.history.replaceState).toBe(originalReplace);
    win.dispatchEvent(new Event("hashchange"));
    expect(win.location.reload).toHaveBeenCalledTimes(1);
  });

  it("stops recording on form/chat interaction without reloading or losing visitor input", () => {
    const { install, accept, win, tracking } = fixture();
    install();
    accept();
    const input = new FakeElement();
    input.sensitive = true;
    const event = new Event("focusin");
    Object.defineProperty(event, "target", { value: input });
    win.dispatchEvent(event);
    expect(tracking.clarity.q.at(-1)).toEqual(["stop"]);
    expect(tracking["ga-disable-G-TEST123456"]).toBe(true);
    expect(win.location.reload).not.toHaveBeenCalled();
  });

  it("does not attach listeners or wrap history when disabled", () => {
    const { win, doc } = fixture();
    const original = win.history.pushState;
    installBrowserTracking(
      win as unknown as Window,
      doc as unknown as Document,
      {},
    )();
    expect(win.history.pushState).toBe(original);
  });

  it("never turns a forbidden cross-origin history call into navigation", () => {
    const { win, install, accept } = fixture();
    const original = win.history.replaceState;
    install();
    accept();
    (win.history as unknown as History).replaceState(
      {},
      "",
      "https://other.test/",
    );
    expect(original).toHaveBeenCalledWith({}, "", "https://other.test/");
    expect(win.location.assign).not.toHaveBeenCalled();
  });

  it("intercepts same-origin link navigation before the SPA renders another route", () => {
    const { install, accept, win, tracking } = fixture();
    install();
    accept();
    const anchor = new FakeElement();
    anchor.anchor = true;
    anchor.href = "https://www.triodelab.no/kontakt";
    const event = new Event("click", { cancelable: true });
    Object.defineProperty(event, "target", { value: anchor });
    win.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    expect(tracking.clarity.q.at(-1)).toEqual(["stop"]);
    expect(win.location.assign).toHaveBeenCalledWith(anchor.href);
  });

  it.each([
    "external",
    "blank",
    "download",
    "modified",
    "not-anchor",
    "not-element",
  ])("preserves unrelated click behavior: %s", (kind) => {
    const { install, accept, win, doc } = fixture();
    install();
    accept();
    const anchor = new FakeElement();
    anchor.anchor = kind !== "not-anchor";
    anchor.href =
      kind === "external"
        ? "https://other.test/"
        : "https://www.triodelab.no/kontakt";
    anchor.target = kind === "blank" ? "_blank" : "";
    if (kind === "download") anchor.setAttribute("download", "file");
    const event = new Event("click", { cancelable: true });
    Object.defineProperty(event, "target", {
      value: kind === "not-element" ? doc : anchor,
    });
    if (kind === "modified")
      Object.defineProperty(event, "ctrlKey", { value: true });
    win.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    expect(win.location.assign).not.toHaveBeenCalled();
  });

  it("handles a blocked provider script without retrying or granting more consent", () => {
    const { install, accept, scripts } = fixture();
    install();
    accept();
    scripts[0].dispatchEvent(new Event("error"));
    expect(scripts[0].attributes.get("data-tracking-state")).toBe(
      "unavailable",
    );
    expect(scripts).toHaveLength(2);
  });

  it("allows initial opt-in through Cookiebot's own controls, but guards their metadata after startup", () => {
    const { install, accept, win, tracking, scripts } = fixture();
    install();
    const checkbox = new FakeElement();
    checkbox.sensitive = true;
    checkbox.consentControl = true;
    const event = new Event("pointerdown");
    Object.defineProperty(event, "target", { value: checkbox });
    win.dispatchEvent(event);
    accept();
    expect(scripts).toHaveLength(2);
    expect(tracking["ga-disable-G-TEST123456"]).toBe(false);
    win.dispatchEvent(event);
    expect(tracking["ga-disable-G-TEST123456"]).toBe(true);
    win.Cookiebot.consent.statistics = false;
    win.dispatchEvent(new Event("CookiebotOnDecline"));
    expect(win.location.reload).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["outbound", "https://external.test/private-token", ""],
    ["mailto", "mailto:private@example.com?body=private", ""],
    ["phone", "tel:+4712345678", ""],
    ["download", "https://www.triodelab.no/private-document.pdf", ""],
    ["newtab", "https://www.triodelab.no/kontakt", "_blank"],
    ["modified", "https://www.triodelab.no/kontakt", ""],
  ])(
    "disables before a later vendor listener could read %s link metadata, without blocking navigation",
    (kind, href, target) => {
      const { install, accept, win, tracking } = fixture();
      install();
      accept();
      let vendorSawDisabled: boolean | undefined;
      win.addEventListener(
        "pointerdown",
        () => {
          vendorSawDisabled = tracking["ga-disable-G-TEST123456"];
        },
        true,
      );
      const anchor = new FakeElement();
      anchor.anchor = true;
      anchor.href = href;
      anchor.target = target;
      if (kind === "download") anchor.setAttribute("download", "document");
      const event = new Event("pointerdown", { cancelable: true });
      Object.defineProperty(event, "target", { value: anchor });
      if (kind === "modified")
        Object.defineProperty(event, "ctrlKey", { value: true });
      win.dispatchEvent(event);
      expect(vendorSawDisabled).toBe(true);
      expect(event.defaultPrevented).toBe(false);
      expect(win.location.assign).not.toHaveBeenCalled();
      expect(win.location.reload).not.toHaveBeenCalled();
      expect(tracking.dataLayer.at(-1)?.[0]).toBe("consent");
      accept();
      expect(tracking["ga-disable-G-TEST123456"]).toBe(true);
    },
  );
});
