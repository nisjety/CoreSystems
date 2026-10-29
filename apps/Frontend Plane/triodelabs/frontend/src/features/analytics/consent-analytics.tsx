"use client";

import { useEffect } from "react";
import { usePathname } from "next/navigation";
import { installBrowserTracking } from "./browser-tracking";
import { triodelabTrackingConfig } from "./site-tracking-config";

const config = triodelabTrackingConfig({
  enabled: process.env.NEXT_PUBLIC_ANALYTICS_ENABLED,
  measurementId: process.env.NEXT_PUBLIC_GA4_MEASUREMENT_ID,
  clarityProjectId: process.env.NEXT_PUBLIC_CLARITY_PROJECT_ID,
});

/** No third-party requests before an explicit Cookiebot Statistics opt-in. */
export function ConsentAnalytics() {
  const pathname = usePathname();
  useEffect(() => installBrowserTracking(window, document, config), [pathname]);
  return null;
}
