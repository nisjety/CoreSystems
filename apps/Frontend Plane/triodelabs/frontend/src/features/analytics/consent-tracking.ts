/** Public installation identifiers only. OAuth grants and export tokens never belong here. */
export type TrackingConfig = Readonly<{
  measurementId?: string;
  clarityProjectId?: string;
}>;

type ConfigInput = Readonly<{
  enabled?: string;
  measurementId?: string;
  clarityProjectId?: string;
}>;

export function trackingConfig(input: ConfigInput): TrackingConfig {
  if (input.enabled !== "true") return {};
  return {
    ...(input.measurementId && /^G-[A-Z0-9]{6,20}$/.test(input.measurementId)
      ? { measurementId: input.measurementId }
      : {}),
    ...(input.clarityProjectId &&
    /^[a-z0-9]{6,20}$/.test(input.clarityProjectId)
      ? { clarityProjectId: input.clarityProjectId }
      : {}),
  };
}

// Deliberately exclude contact, legal, API, account and unknown/dynamic routes.
const PUBLIC_PATHS = new Set([
  "/",
  "/en",
  "/en/about",
  "/en/platform",
  "/en/services",
  "/en/services/web-solutions",
  "/en/services/ecommerce",
  "/en/services/organic-traffic",
  "/en/services/integrations",
  "/en/services/google-and-meta-ads",
  "/en/services/ai-and-automation",
  "/en/services/advisory-and-consulting",
  "/om-oss",
  "/plattform",
  "/innsikt",
  "/innsikt/systemintegrasjon-for-nettbutikk",
  "/innsikt/ki-pilot-for-bedrifter",
  "/innsikt/google-meta-tiktok-snapchat-annonsering",
  "/tjenester",
  "/tjenester/web-losninger",
  "/tjenester/nettbutikk",
  "/tjenester/google-og-meta-ads",
  "/tjenester/organisk-trafikk",
  "/tjenester/ki-og-automatisering",
  "/tjenester/integrasjoner",
  "/tjenester/radgivning-og-konsulentbistand",
]);

export function publicPage(value: string) {
  try {
    const url = new URL(value);
    if (
      url.protocol !== "https:" ||
      url.port ||
      url.username ||
      url.password ||
      !["triodelab.no", "www.triodelab.no"].includes(url.hostname) ||
      !PUBLIC_PATHS.has(url.pathname)
    )
      return null;
    return {
      location: url.origin + url.pathname,
      title: `TriodeLab ${url.pathname}`,
    };
  } catch {
    return null;
  }
}

export interface TrackingPort {
  url(): string;
  referrer(): string;
  statistics(): boolean;
  privacySignal(): boolean;
  loadScript(id: string, src: string): void;
  google(...args: unknown[]): void;
  clarity(...args: unknown[]): void;
  disableGoogle(id: string, disabled: boolean): void;
  clearCookies(): void;
  reload(): void;
}

const DENIED = {
  analytics_storage: "denied",
  ad_storage: "denied",
  ad_user_data: "denied",
  ad_personalization: "denied",
} as const;

function cleanTrackingUrl(value: string): boolean {
  if (!value) return true;
  try {
    const url = new URL(value);
    // Enhanced Measurement and Clarity read the actual URL/referrer themselves.
    // Config overrides do not sanitize their search, link or form metadata.
    return !url.search && !url.hash && !url.username && !url.password;
  } catch {
    return false;
  }
}

const SEARCH_REFERRER_HOSTS = new Set([
  "google.com",
  "www.google.com",
  "google.no",
  "www.google.no",
  "bing.com",
  "www.bing.com",
  "duckduckgo.com",
  "www.duckduckgo.com",
]);

function trackingReferrer(value: string): string | null {
  if (!value) return "";
  if (!cleanTrackingUrl(value)) return null;
  const publicReferrer = publicPage(value);
  if (publicReferrer) return publicReferrer.location;
  // Exact HTTPS origin roots only: no search terms, paths, credentials, ports,
  // lookalike domains or arbitrary external referrers enter automatic telemetry.
  if (!/^https:\/\/[a-z0-9.-]+\/?$/i.test(value)) return null;
  const url = new URL(value);
  return SEARCH_REFERRER_HOSTS.has(url.hostname) ? `${url.origin}/` : null;
}

export function createConsentTracking(
  config: TrackingConfig,
  port: TrackingPort,
) {
  let googleStarted = false;
  let clarityStarted = false;
  let stopped = false;
  let withdrawalHandled = false;

  const stop = (withdrawn: boolean) => {
    if (!googleStarted && !clarityStarted) return;
    const cleanWithdrawal = withdrawn && !withdrawalHandled;
    if (!stopped) {
      stopped = true;
      if (googleStarted && config.measurementId) {
        port.disableGoogle(config.measurementId, true);
        port.google("consent", "update", DENIED);
      }
      if (clarityStarted) {
        if (cleanWithdrawal)
          port.clarity("consentv2", {
            analytics_Storage: "denied",
            ad_Storage: "denied",
          });
        // Microsoft clarity-js/src/clarity.ts stop() disconnects observers,
        // interaction modules and history listeners. Removing the tag is not enough.
        port.clarity("stop");
      }
    } else if (cleanWithdrawal && clarityStarted) {
      port.clarity("consentv2", {
        analytics_Storage: "denied",
        ad_Storage: "denied",
      });
      port.clarity("stop");
    }
    if (cleanWithdrawal) {
      withdrawalHandled = true;
      port.clearCookies();
      // Recreate the JS realm: denied consent alone still allows cookieless Clarity.
      port.reload();
    }
  };

  const reconcile = () => {
    const allowed = port.statistics() && !port.privacySignal();
    if (!allowed) {
      stop(true);
      return;
    }
    if (stopped) return;
    const page = publicPage(port.url());
    const referrer = trackingReferrer(port.referrer());
    if (!page || !cleanTrackingUrl(port.url()) || referrer === null) {
      stop(false);
      return;
    }
    if (config.measurementId && !googleStarted) {
      googleStarted = true;
      port.disableGoogle(config.measurementId, false);
      port.google("consent", "default", DENIED);
      port.google("consent", "update", {
        ...DENIED,
        analytics_storage: "granted",
      });
      port.google("js", new Date());
      port.google("config", config.measurementId, {
        send_page_view: true,
        allow_google_signals: false,
        allow_ad_personalization_signals: false,
        page_location: page.location,
        page_referrer: referrer,
        page_title: page.title,
      });
      // Enhanced Measurement stays unchanged. One automatic config per document;
      // do not add a manual page_view alongside Google's automatic pageviews.
      port.loadScript(
        "triodelab-ga4",
        `https://www.googletagmanager.com/gtag/js?id=${config.measurementId}`,
      );
    }
    if (config.clarityProjectId && !clarityStarted) {
      clarityStarted = true;
      port.clarity("consentv2", {
        analytics_Storage: "granted",
        ad_Storage: "denied",
      });
      port.loadScript(
        "triodelab-clarity",
        `https://www.clarity.ms/tag/${config.clarityProjectId}`,
      );
    }
  };

  return {
    reconcile,
    beforeNavigation(target: string) {
      if ((!googleStarted && !clarityStarted) || target === port.url())
        return false;
      stop(false);
      return true;
    },
    sensitiveInteraction(blockFutureStart = true) {
      // Disable before vendor listeners can inspect private interaction metadata.
      // Do not reload, submit, interrupt typing or change the visitor's CMP choice.
      stop(false);
      // A form/chat opened before opt-in must not later start tracking this document.
      if (blockFutureStart) stopped = true;
    },
  };
}

export type ConsentTracking = ReturnType<typeof createConsentTracking>;
