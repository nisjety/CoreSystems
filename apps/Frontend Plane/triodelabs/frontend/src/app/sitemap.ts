import type { MetadataRoute } from "next";
import { insights } from "../features/insights/insights-content";
import { htmlLang } from "../lib/i18n/locales";
import { localizedRoutes } from "../lib/i18n/routes";
import { toAbsoluteUrl } from "../lib/seo";

// Every page is listed in both languages, each with its hreflang alternates.
export default function sitemap(): MetadataRoute.Sitemap {
  const localizedEntries = localizedRoutes.flatMap((route) => {
    const languages = {
      [htmlLang.nb]: toAbsoluteUrl(route.nb),
      [htmlLang.en]: toAbsoluteUrl(route.en),
      "x-default": toAbsoluteUrl(route.nb),
    };

    return [route.nb, route.en].map((path) => ({
      url: toAbsoluteUrl(path),
      alternates: { languages },
    }));
  });

  const norwegianOnlyPaths = ["/innsikt", ...insights.map(({ canonical }) => canonical)];
  const norwegianOnlyEntries = norwegianOnlyPaths.map((path) => ({
    url: toAbsoluteUrl(path),
    alternates: {
      languages: {
        [htmlLang.nb]: toAbsoluteUrl(path),
        "x-default": toAbsoluteUrl(path),
      },
    },
  }));

  return [...localizedEntries, ...norwegianOnlyEntries];
}
