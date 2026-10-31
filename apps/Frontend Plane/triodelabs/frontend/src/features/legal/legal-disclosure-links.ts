import { createElement } from "react";
import type { LegalDocument } from "./legal-content";

/** Static, reviewed provider disclosures, rendered as text links rather than HTML. */
export function LegalDisclosureLinks({
  links,
}: {
  links: NonNullable<LegalDocument["sections"][number]["links"]>;
}) {
  return createElement(
    "ul",
    { className: "mt-4 list-none space-y-2 font-body text-base sm:text-lg" },
    links.map((link) =>
      createElement(
        "li",
        { key: link.href },
        createElement(
          "a",
          {
            href: link.href,
            rel: "noreferrer",
            className: "underline underline-offset-4",
          },
          link.label,
        ),
      ),
    ),
  );
}
