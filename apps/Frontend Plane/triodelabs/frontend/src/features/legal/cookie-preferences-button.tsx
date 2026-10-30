"use client";

import { useState } from "react";
import type { Locale } from "@/lib/i18n/locales";
import { openCookiePreferences } from "./cookie-preferences";

const labels: Record<Locale, { button: string; unavailable: string }> = {
  nb: {
    button: "Endre samtykkeinnstillinger",
    unavailable:
      "Samtykkeinnstillingene er ikke tilgjengelige akkurat nå. Prøv igjen når samtykkebanneret er lastet, eller slett kapslene i nettleseren.",
  },
  en: {
    button: "Change cookie preferences",
    unavailable:
      "Cookie preferences are not available right now. Try again after the consent banner loads, or clear the cookies in your browser.",
  },
};

export function CookiePreferencesButton({ locale }: { locale: Locale }) {
  const [failed, setFailed] = useState(false);
  return (
    <div className="my-6">
      <button
        type="button"
        className="rounded-full border border-[#D9D9D4] px-5 py-3 font-label text-sm underline-offset-4 hover:underline focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4"
        onClick={() => {
          const manager = (
            window as Window & {
              Cookiebot?: { renew?: () => void };
            }
          ).Cookiebot;
          setFailed(!openCookiePreferences(manager));
        }}
      >
        {labels[locale].button}
      </button>
      {failed && (
        <p role="status" className="mt-3 text-sm">
          {labels[locale].unavailable}
        </p>
      )}
    </div>
  );
}
