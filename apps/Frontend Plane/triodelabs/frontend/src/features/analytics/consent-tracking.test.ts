import { describe, expect, it, vi } from "vitest";
import {
  createConsentTracking,
  publicPage,
  trackingConfig,
  type TrackingPort,
} from "./consent-tracking";

const enabled = {
  enabled: "true",
  measurementId: "G-TEST123456",
  clarityProjectId: "test123456",
};

function fixture(overrides: Partial<TrackingPort> = {}) {
  const state = { url: "https://www.triodelab.no/", consent: false };
  const port: TrackingPort = {
    url: () => state.url,
    referrer: () => "",
    statistics: () => state.consent,
    privacySignal: () => false,
    loadScript: vi.fn(),
    google: vi.fn(),
    clarity: vi.fn(),
    disableGoogle: vi.fn(),
    clearCookies: vi.fn(),
    reload: vi.fn(),
    ...overrides,
  };
  return {
    state,
    port,
    runtime: createConsentTracking(trackingConfig(enabled), port),
  };
}

describe("public tracking configuration", () => {
  it("requires explicit activation and valid public IDs", () => {
    expect(trackingConfig({})).toEqual({});
    expect(trackingConfig({ ...enabled, enabled: "false" })).toEqual({});
    expect(
      trackingConfig({
        ...enabled,
        measurementId: "G-X<script>",
        clarityProjectId: "../token",
      }),
    ).toEqual({});
    expect(trackingConfig(enabled)).toEqual({
      measurementId: "G-TEST123456",
      clarityProjectId: "test123456",
    });
  });

  it("allows independent providers without falling back to another site's IDs", () => {
    expect(
      trackingConfig({ enabled: "true", measurementId: enabled.measurementId }),
    ).toEqual({ measurementId: enabled.measurementId });
    expect(
      trackingConfig({
        enabled: "true",
        clarityProjectId: enabled.clarityProjectId,
      }),
    ).toEqual({ clarityProjectId: enabled.clarityProjectId });
  });

  it("sanitizes public URLs and rejects unknown, private or nonproduction routes", () => {
    expect(
      publicPage(
        "https://www.triodelab.no/tjenester?email=private@example.com#secret",
      ),
    ).toEqual({
      location: "https://www.triodelab.no/tjenester",
      title: "TriodeLab /tjenester",
    });
    for (const path of [
      "kontakt",
      "account",
      "api/contact",
      "innsikt/private@example.com",
      "personvern",
    ]) {
      expect(publicPage(`https://www.triodelab.no/${path}`)).toBeNull();
    }
    for (const url of [
      "not-a-url",
      "http://www.triodelab.no/",
      "https://triodelab.no.evil.test/",
      "https://preview.vercel.app/",
      "http://localhost:3000/",
    ]) {
      expect(publicPage(url)).toBeNull();
    }
  });

  it("allows only approved public English routes for analytics", () => {
    expect(
      publicPage("https://www.triodelab.no/en/services/web-solutions"),
    ).toEqual({
      location: "https://www.triodelab.no/en/services/web-solutions",
      title: "TriodeLab /en/services/web-solutions",
    });
    expect(publicPage("https://www.triodelab.no/en/about")).not.toBeNull();
    expect(
      publicPage(
        "https://www.triodelab.no/innsikt/systemintegrasjon-for-nettbutikk",
      ),
    ).not.toBeNull();
    for (const path of [
      "/en/contact",
      "/en/privacy",
      "/en/cookies",
      "/en/terms",
    ]) {
      expect(publicPage(`https://www.triodelab.no${path}`)).toBeNull();
    }
  });
});

describe("consent-gated tracking", () => {
  it("does not load scripts or queue telemetry without Statistics opt-in", () => {
    const { runtime, port } = fixture();
    runtime.reconcile();
    expect(port.loadScript).not.toHaveBeenCalled();
    expect(port.google).not.toHaveBeenCalled();
    expect(port.clarity).not.toHaveBeenCalled();
  });

  it("accepts stored consent and never implicitly grants advertising", () => {
    const { runtime, state, port } = fixture();
    state.consent = true;
    runtime.reconcile();
    expect(port.loadScript).toHaveBeenCalledWith(
      "triodelab-ga4",
      "https://www.googletagmanager.com/gtag/js?id=G-TEST123456",
    );
    expect(port.loadScript).toHaveBeenCalledWith(
      "triodelab-clarity",
      "https://www.clarity.ms/tag/test123456",
    );
    expect(port.google).toHaveBeenCalledWith("consent", "update", {
      analytics_storage: "granted",
      ad_storage: "denied",
      ad_user_data: "denied",
      ad_personalization: "denied",
    });
    expect(port.google).toHaveBeenCalledWith(
      "config",
      enabled.measurementId,
      expect.objectContaining({
        send_page_view: true,
        allow_google_signals: false,
        allow_ad_personalization_signals: false,
      }),
    );
    expect(port.clarity).toHaveBeenCalledWith("consentv2", {
      analytics_Storage: "granted",
      ad_Storage: "denied",
    });
  });

  it("loads once after later opt-in, including repeated ready/accept events and StrictMode setup", () => {
    const { runtime, state, port } = fixture();
    runtime.reconcile();
    state.consent = true;
    runtime.reconcile();
    runtime.reconcile();
    expect(port.loadScript).toHaveBeenCalledTimes(2);
    expect(
      vi
        .mocked(port.google)
        .mock.calls.filter(([command]) => command === "config"),
    ).toHaveLength(1);
  });

  it("does not send a custom page_view alongside automatic configuration", () => {
    const { runtime, state, port } = fixture();
    state.consent = true;
    runtime.reconcile();
    expect(port.google).toHaveBeenCalledWith("config", enabled.measurementId, {
      send_page_view: true,
      allow_google_signals: false,
      allow_ad_personalization_signals: false,
      page_location: "https://www.triodelab.no/",
      page_referrer: "",
      page_title: "TriodeLab /",
    });
    expect(
      vi
        .mocked(port.google)
        .mock.calls.some(([command]) => command === "event"),
    ).toBe(false);
  });

  it("blocks all tracking rather than merely sanitizing a private URL/referrer", () => {
    const { runtime, state, port } = fixture({
      referrer: () => "https://external.test/?email=private@example.com#token",
    });
    state.url =
      "https://www.triodelab.no/tjenester?email=private@example.com#token";
    state.consent = true;
    runtime.reconcile();
    expect(port.google).not.toHaveBeenCalled();
    expect(port.loadScript).not.toHaveBeenCalled();
    expect(port.clarity).not.toHaveBeenCalled();
    expect(JSON.stringify(vi.mocked(port.google).mock.calls)).not.toContain(
      "private@example.com",
    );
  });

  it.each([
    "https://www.triodelab.no/?token=secret",
    "https://www.triodelab.no/#secret",
  ])("does not start either provider on URLs with query/hash: %s", (url) => {
    const { runtime, state, port } = fixture();
    state.url = url;
    state.consent = true;
    runtime.reconcile();
    expect(port.loadScript).not.toHaveBeenCalled();
    expect(port.google).not.toHaveBeenCalled();
    expect(port.clarity).not.toHaveBeenCalled();
  });

  it("does not start Clarity with a potentially private referrer", () => {
    const { runtime, state, port } = fixture({
      referrer: () =>
        "https://www.triodelab.no/kontakt?email=private@example.com",
    });
    state.consent = true;
    runtime.reconcile();
    expect(port.clarity).not.toHaveBeenCalled();
    expect(port.google).not.toHaveBeenCalled();
    expect(port.loadScript).not.toHaveBeenCalled();
  });

  it.each([
    "https://external.test/reset/secret-token",
    "https://www.triodelab.no/users/private@example.com",
    "invalid-referrer",
  ])("does not leak a private path-only referrer: %s", (referrer) => {
    const { runtime, state, port } = fixture({ referrer: () => referrer });
    state.consent = true;
    runtime.reconcile();
    expect(port.clarity).not.toHaveBeenCalled();
    expect(port.google).not.toHaveBeenCalled();
    expect(port.loadScript).not.toHaveBeenCalled();
  });

  it("permits a known public referrer without query or fragment", () => {
    const { runtime, state, port } = fixture({
      referrer: () => "https://www.triodelab.no/tjenester",
    });
    state.consent = true;
    runtime.reconcile();
    expect(port.clarity).toHaveBeenCalledWith("consentv2", {
      analytics_Storage: "granted",
      ad_Storage: "denied",
    });
    expect(port.google).toHaveBeenCalledWith(
      "config",
      enabled.measurementId,
      expect.objectContaining({
        page_referrer: "https://www.triodelab.no/tjenester",
      }),
    );
  });

  it.each([
    "google.com",
    "www.google.com",
    "google.no",
    "www.google.no",
    "bing.com",
    "www.bing.com",
    "duckduckgo.com",
    "www.duckduckgo.com",
  ])(
    "permits only a clean root search referrer and preserves attribution: %s",
    (host) => {
      const referrer = `https://${host}/`;
      const { runtime, state, port } = fixture({ referrer: () => referrer });
      state.consent = true;
      runtime.reconcile();
      expect(port.loadScript).toHaveBeenCalledWith(
        "triodelab-ga4",
        expect.any(String),
      );
      expect(port.google).toHaveBeenCalledWith(
        "config",
        enabled.measurementId,
        expect.objectContaining({ page_referrer: referrer }),
      );
    },
  );

  it("normalizes a permitted search origin without inventing a path", () => {
    const { runtime, state, port } = fixture({
      referrer: () => "https://google.no",
    });
    state.consent = true;
    runtime.reconcile();
    expect(port.google).toHaveBeenCalledWith(
      "config",
      enabled.measurementId,
      expect.objectContaining({ page_referrer: "https://google.no/" }),
    );
  });

  it.each([
    "https://google.com.evil.test/",
    "https://evilgoogle.com/",
    "https://accounts.google.com/",
    "https://www.google.no.evil.test/",
    "https://bıng.com/",
    "https://duckduckgo.com.evil.test/",
    "https://google.com/search",
    "https://www.bing.com/private/token",
    "https://duckduckgo.com/?q=private",
    "https://google.com/?",
    "https://google.no/#private",
    "https://bing.com/#",
    "https://private@google.com/",
    "https://private:token@www.google.no/",
    "https://google.com:8443/",
    "https://google.com:443/",
    "http://google.com/",
    "https://google.com\\",
    "https://external.test/",
  ])(
    "blocks an unsafe or non-allowlisted search-like referrer: %s",
    (referrer) => {
      const { runtime, state, port } = fixture({ referrer: () => referrer });
      state.consent = true;
      runtime.reconcile();
      expect(port.loadScript).not.toHaveBeenCalled();
      expect(port.google).not.toHaveBeenCalled();
      expect(port.clarity).not.toHaveBeenCalled();
    },
  );

  it.each([
    "https://www.triodelab.no/kontakt",
    "https://www.triodelab.no/account",
  ])("never starts on excluded routes: %s", (url) => {
    const { runtime, state, port } = fixture();
    state.url = url;
    state.consent = true;
    runtime.reconcile();
    expect(port.loadScript).not.toHaveBeenCalled();
  });

  it("honors browser privacy signals even with stored Statistics consent", () => {
    const { runtime, state, port } = fixture({ privacySignal: () => true });
    state.consent = true;
    runtime.reconcile();
    expect(port.loadScript).not.toHaveBeenCalled();
  });

  it("fully disables and reloads on revocation, rather than just removing script elements", () => {
    const { runtime, state, port } = fixture();
    state.consent = true;
    runtime.reconcile();
    state.consent = false;
    runtime.reconcile();
    runtime.reconcile();
    expect(port.disableGoogle).toHaveBeenLastCalledWith(
      enabled.measurementId,
      true,
    );
    expect(port.clarity).toHaveBeenCalledWith("consentv2", {
      analytics_Storage: "denied",
      ad_Storage: "denied",
    });
    expect(port.clarity).toHaveBeenCalledWith("stop");
    expect(port.clearCookies).toHaveBeenCalledTimes(1);
    expect(port.reload).toHaveBeenCalledTimes(1);
  });

  it("stops before a history change and does not restart on private SPA routes", () => {
    const { runtime, state, port } = fixture();
    state.consent = true;
    runtime.reconcile();
    expect(runtime.beforeNavigation("https://www.triodelab.no/kontakt")).toBe(
      true,
    );
    state.url = "https://www.triodelab.no/kontakt";
    runtime.reconcile();
    expect(port.clarity).toHaveBeenLastCalledWith("stop");
    expect(port.disableGoogle).toHaveBeenLastCalledWith(
      enabled.measurementId,
      true,
    );
    expect(port.loadScript).toHaveBeenCalledTimes(2);
  });

  it("does not interfere with navigation when disabled or the URL is unchanged", () => {
    const { runtime, port, state } = fixture();
    expect(runtime.beforeNavigation("https://www.triodelab.no/kontakt")).toBe(
      false,
    );
    state.consent = true;
    runtime.reconcile();
    expect(runtime.beforeNavigation(state.url)).toBe(false);
    expect(port.clarity).not.toHaveBeenCalledWith("stop");
  });

  it("permanently stops both providers before chat/form interaction without interrupting input", () => {
    const { runtime, state, port } = fixture();
    state.consent = true;
    runtime.reconcile();
    runtime.sensitiveInteraction();
    runtime.sensitiveInteraction();
    runtime.reconcile();
    expect(port.clarity).toHaveBeenCalledWith("stop");
    expect(port.disableGoogle).toHaveBeenLastCalledWith(
      enabled.measurementId,
      true,
    );
    expect(port.google).toHaveBeenLastCalledWith("consent", "update", {
      analytics_storage: "denied",
      ad_storage: "denied",
      ad_user_data: "denied",
      ad_personalization: "denied",
    });
    expect(port.reload).not.toHaveBeenCalled();
    expect(port.loadScript).toHaveBeenCalledTimes(2);
  });

  it("does not start later in a document where private interaction happened before consent", () => {
    const { runtime, state, port } = fixture();
    runtime.sensitiveInteraction();
    state.consent = true;
    runtime.reconcile();
    expect(port.loadScript).not.toHaveBeenCalled();
    expect(port.google).not.toHaveBeenCalled();
  });

  it("does not mistake CMP choices before opt-in for a private form interaction", () => {
    const { runtime, state, port } = fixture();
    runtime.sensitiveInteraction(false);
    state.consent = true;
    runtime.reconcile();
    expect(port.loadScript).toHaveBeenCalledTimes(2);
  });

  it("still clears cookies and reloads once when consent is withdrawn after a private-interaction stop", () => {
    const { runtime, state, port } = fixture();
    state.consent = true;
    runtime.reconcile();
    runtime.sensitiveInteraction();
    state.consent = false;
    runtime.reconcile();
    runtime.reconcile();
    expect(port.clearCookies).toHaveBeenCalledTimes(1);
    expect(port.reload).toHaveBeenCalledTimes(1);
  });
});
