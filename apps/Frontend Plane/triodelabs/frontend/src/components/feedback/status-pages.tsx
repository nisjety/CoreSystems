"use client";

import { useEffect } from "react";
import Link from "next/link";
import { Button } from "@/components/ui/button";
import { ArrowLeft, Home, RotateCcw } from "@/components/icons";
import type { Locale, Localized } from "@/lib/i18n/locales";
import { localizeHref } from "@/lib/i18n/routes";

const copy: Localized<{
  contact: string;
  error: { body: string; help: string; retry: string; title: string };
  home: string;
  loading: string;
  notFound: { back: string; body: string; help: string; title: string };
}> = {
  nb: {
    home: "Hjem",
    contact: "Kontakt oss",
    loading: "Laster inn...",
    error: {
      title: "Noe gikk galt",
      body: "Beklager, det oppstod en feil. Dette kan være et midlertidig problem. Prøv å laste siden på nytt eller gå tilbake til hovedsiden.",
      retry: "Prøv igjen",
      help: "Problemet vedvarer?",
    },
    notFound: {
      title: "Side ikke funnet",
      body: "Beklager, siden du leter etter eksisterer ikke. Den kan ha blitt flyttet, slettet eller du kan ha skrevet feil URL.",
      back: "Gå tilbake",
      help: "Trenger du hjelp?",
    },
  },
  en: {
    home: "Home",
    contact: "Contact us",
    loading: "Loading...",
    error: {
      title: "Something went wrong",
      body: "Sorry, an error occurred. This may be a temporary problem. Try reloading the page or go back to the home page.",
      retry: "Try again",
      help: "Still having problems?",
    },
    notFound: {
      title: "Page not found",
      body: "Sorry, the page you are looking for does not exist. It may have been moved or deleted, or the URL may be mistyped.",
      back: "Go back",
      help: "Need help?",
    },
  },
};

function HelpLink({ label, locale }: { label: string; locale: Locale }) {
  return (
    <p className="text-sm text-slate-500 mt-8">
      {label}{" "}
      <Link
        href={localizeHref("/kontakt", locale)}
        className="text-black hover:underline font-medium"
      >
        {copy[locale].contact}
      </Link>
    </p>
  );
}

export function ErrorView({
  error,
  locale,
  reset,
}: {
  error: Error & { digest?: string };
  locale: Locale;
  reset: () => void;
}) {
  const text = copy[locale];

  useEffect(() => {
    console.error(error);
  }, [error]);

  return (
    <main
      id="main-content"
      className="min-h-screen bg-white flex items-center justify-center px-4"
    >
      <div className="text-center max-w-md mx-auto">
        <div className="w-16 h-16 bg-red-100 rounded-full flex items-center justify-center mx-auto mb-6">
          <RotateCcw className="w-8 h-8 text-red-600" />
        </div>

        <h1 className="text-2xl md:text-3xl font-bold text-black mb-4">
          {text.error.title}
        </h1>
        <p className="text-slate-600 mb-8 leading-relaxed">{text.error.body}</p>

        <div className="flex flex-col sm:flex-row items-center justify-center gap-4">
          <Button
            onClick={reset}
            className="bg-black hover:bg-slate-800 text-white border-none px-6 py-3 font-semibold rounded-full transition-all duration-300"
          >
            <RotateCcw className="w-4 h-4 mr-2" />
            {text.error.retry}
          </Button>

          <Link href={localizeHref("/", locale)}>
            <Button
              variant="outline"
              className="border-2 border-black text-black hover:bg-black hover:text-white px-6 py-3 font-semibold rounded-full transition-all duration-300"
            >
              <Home className="w-4 h-4 mr-2" />
              {text.home}
            </Button>
          </Link>
        </div>

        <HelpLink label={text.error.help} locale={locale} />
      </div>
    </main>
  );
}

export function NotFoundView({ locale }: { locale: Locale }) {
  const text = copy[locale];

  return (
    <main
      id="main-content"
      className="min-h-screen bg-white flex items-center justify-center px-4"
    >
      <div className="text-center max-w-md mx-auto">
        <div className="text-8xl md:text-9xl font-black text-slate-200 mb-8">
          404
        </div>

        <h1 className="text-2xl md:text-3xl font-bold text-black mb-4">
          {text.notFound.title}
        </h1>
        <p className="text-slate-600 mb-8 leading-relaxed">
          {text.notFound.body}
        </p>

        <div className="flex flex-col sm:flex-row items-center justify-center gap-4">
          <Button
            onClick={() => window.history.back()}
            variant="outline"
            className="border-2 border-black text-black hover:bg-black hover:text-white px-6 py-3 font-semibold rounded-full transition-all duration-300"
          >
            <ArrowLeft className="w-4 h-4 mr-2" />
            {text.notFound.back}
          </Button>

          <Link href={localizeHref("/", locale)}>
            <Button className="bg-black hover:bg-slate-800 text-white border-none px-6 py-3 font-semibold rounded-full transition-all duration-300">
              <Home className="w-4 h-4 mr-2" />
              {text.home}
            </Button>
          </Link>
        </div>

        <HelpLink label={text.notFound.help} locale={locale} />
      </div>
    </main>
  );
}

export function LoadingView({ locale }: { locale: Locale }) {
  return (
    <div className="min-h-screen bg-white flex items-center justify-center">
      <div className="text-center">
        <div className="relative w-16 h-16 mx-auto mb-6">
          <div className="absolute inset-0 border-4 border-gray-200 rounded-full"></div>
          <div className="absolute inset-0 border-4 border-black rounded-full border-t-transparent animate-spin"></div>
        </div>

        <h2 className="text-xl font-semibold text-gray-900 mb-2">
          {copy[locale].loading}
        </h2>
        <p className="text-gray-600">TriodeLab</p>
      </div>
    </div>
  );
}
