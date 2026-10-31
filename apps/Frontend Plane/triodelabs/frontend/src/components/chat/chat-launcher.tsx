"use client";

import Script from "next/script";
import { useState } from "react";
import { MessageCircle } from "lucide-react";
import type { Locale } from "@/lib/i18n/locales";

const copy = {
  nb: {
    errorBefore: "Chatten kunne ikke lastes. Last siden på nytt for å prøve igjen, eller",
    errorLink: "kontakt oss på e-post",
    loadingLabel: "Laster chat",
    loading: "Laster chat …",
    openLabel: "Aktiver chat fra Agenci",
    open: "Aktiver chat",
  },
  en: {
    errorBefore: "The chat could not be loaded. Reload the page to try again, or",
    errorLink: "email us",
    loadingLabel: "Loading chat",
    loading: "Loading chat …",
    openLabel: "Start the Agenci chat",
    open: "Start chat",
  },
} as const;

/** Load the third-party chat only after the visitor asks to use it. */
export function ChatLauncher({ locale = "nb" }: { locale?: Locale }) {
  const [state, setState] = useState<"idle" | "loading" | "ready" | "error">(
    "idle",
  );
  const text = copy[locale];

  return (
    <>
      {state !== "ready" && (
        <div className="fixed bottom-5 right-5 z-50 flex max-w-[260px] flex-col items-end gap-2">
          {state === "error" && (
            <p
              role="alert"
              className="rounded-lg bg-white p-3 text-sm text-[#161616] shadow-md"
            >
              {text.errorBefore}{" "}
              <a
                className="underline underline-offset-2"
                href="mailto:post@triodelab.no"
              >
                {text.errorLink}
              </a>
              .
            </p>
          )}
          {state !== "error" && (
            <button
              className="flex items-center gap-2 rounded-full bg-[#365D65] px-4 py-3 text-sm text-white shadow-lg transition-colors hover:bg-[#25474E] disabled:opacity-70"
              disabled={state === "loading"}
              onClick={() => setState("loading")}
              type="button"
              aria-label={state === "loading" ? text.loadingLabel : text.openLabel}
            >
              <MessageCircle aria-hidden="true" className="size-5" />
              {state === "loading" ? text.loading : text.open}
            </button>
          )}
        </div>
      )}
      {(state === "loading" || state === "ready") && (
        <Script
          src="https://widget.agenci.no/widget.iife.js"
          data-organization-id="N5zumaVizWadxyo8h6gMQ0BwBtqYvGo7"
          data-agent-id="cmurg6x4j000d01nurf1wnz51"
          strategy="afterInteractive"
          onLoad={() => setState("ready")}
          onError={() => setState("error")}
        />
      )}
    </>
  );
}
