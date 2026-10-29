import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { legalDocumentsByLocale } from "./legal-content-en";
import { LegalDisclosureLinks } from "./legal-disclosure-links";

describe("rendered provider privacy disclosures", () => {
  it.each(["nb", "en"] as const)(
    "renders Microsoft's actual privacy link on the %s privacy and cookie pages",
    (locale) => {
      for (const document of [
        legalDocumentsByLocale[locale].privacy,
        legalDocumentsByLocale[locale].cookies,
      ]) {
        const links = document.sections.flatMap(
          (section) => section.links ?? [],
        );
        const html = renderToStaticMarkup(
          createElement(LegalDisclosureLinks, { links }),
        );
        expect(html).toContain(
          'href="https://privacy.microsoft.com/privacystatement"',
        );
        expect(html).toContain('rel="noreferrer"');
        expect(html).not.toContain("<script");
      }
    },
  );
});
