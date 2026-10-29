"use client";

import { useEffect, useRef, useState } from "react";
import { usePathname } from "next/navigation";
import { installWebVitals } from "./browser-vitals";
import {
  developmentConsentAvailable,
  readDevelopmentConsent,
  setDevelopmentConsent,
  privacySignal,
} from "./development-consent";

type LocalView = {
  accepted: boolean;
  navigated: boolean;
  blocked: boolean;
  unavailable: boolean;
};

export function ConsentWebVitals() {
  const pathname = usePathname();
  const firstUrl = useRef<string | null>(null);
  const navigated = useRef(false);
  const [local, setLocal] = useState<LocalView | null>(null);
  const development = process.env.NODE_ENV === "development";
  useEffect(() => {
    const dispose = installWebVitals(window, {
      enabled: process.env.NEXT_PUBLIC_WEB_VITALS_ENABLED === "true",
      development,
    });
    firstUrl.current ??= window.location.href;
    navigated.current ||= firstUrl.current !== window.location.href;
    setLocal(
      developmentConsentAvailable(window, development)
        ? {
            accepted: readDevelopmentConsent(window, development) === true,
            navigated: navigated.current,
            blocked: privacySignal(window),
            unavailable: false,
          }
        : null,
    );
    const failed = () =>
      setLocal((value) => (value ? { ...value, unavailable: true } : null));
    window.addEventListener("triodelab:web-vitals-unavailable", failed);
    return () => {
      dispose();
      window.removeEventListener("triodelab:web-vitals-unavailable", failed);
    };
  }, [pathname, development]);

  if (!local) return null;
  const changeConsent = () => {
    const accepted = !local.accepted;
    if (setDevelopmentConsent(window, development, accepted))
      setLocal({ ...local, accepted });
  };
  return (
    <aside
      data-testid="local-vitals-consent"
      data-web-vitals-development-consent="true"
      aria-label="Local Web Vitals test consent"
      style={{
        position: "fixed",
        bottom: 16,
        left: 16,
        zIndex: 99999,
        maxWidth: 360,
        padding: 16,
        border: "1px solid #d4d4d4",
        borderRadius: 12,
        background: "#fff",
        color: "#18181b",
        fontSize: 14,
        lineHeight: 1.5,
        boxShadow: "0 4px 24px #0002",
      }}
    >
      <strong>Local Web Vitals test · development only</strong>
      <p>
        Sends LCP, INP and CLS to the local collector only. No Google Analytics
        or Clarity consent. Off by default; resets on reload.
      </p>
      <p role="status">
        {local.blocked
          ? "Blocked by your browser privacy signal."
          : local.navigated
            ? "Navigation stopped this document’s measurements. Reload for a fresh test."
            : local.accepted
              ? "Local measurement allowed for this document."
              : "Local measurement is off."}
      </p>
      {local.unavailable && (
        <p role="alert">
          The local collector could not accept a measurement. Check the local
          server and Dragonfly.
        </p>
      )}
      {local.navigated ? (
        <button type="button" onClick={() => window.location.reload()}>
          Reload for a new test
        </button>
      ) : (
        <button
          type="button"
          disabled={local.blocked}
          onClick={changeConsent}
          style={{
            marginTop: 8,
            padding: "8px 12px",
            background: "#18181b",
            color: "#fff",
            borderRadius: 8,
          }}
        >
          {local.accepted ? "Stop local Web Vitals" : "Allow local Web Vitals"}
        </button>
      )}
      <p style={{ marginTop: 8, fontSize: 12 }}>
        Stopping blocks new reports and reloads the page. Already received
        measurements expire within seven UTC calendar days.
      </p>
    </aside>
  );
}
