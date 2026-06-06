import { describe, expect, it } from "vitest";

import { extractCustomPropertyColors, prioritizeStylesheetUrls } from "./branding-css";

function cssColorToHex(value: string | null | undefined): string | null {
  if (!value) return null;
  const hex = value.match(/#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})\b/i)?.[0];
  return hex ? `#${hex.toLowerCase().replace("#", "").slice(0, 6)}` : null;
}

describe("branding CSS discovery", () => {
  it("prioritizes first-party theme CSS before ecommerce plugin CSS", () => {
    const urls = [
      "https://skinsecret.no/wp-content/plugins/woocommerce/assets/css/woocommerce.css?ver=1.0.51-24",
      "https://skinsecret.no/wp-content/plugins/yith-woocommerce-wishlist/assets/css/style.css?ver=1.0.51-24",
      "https://skinsecret.no/wp-content/themes/skinsecret/style.css?ver=1.0.51-24",
      "https://skinsecret.no/wp-content/plugins/contact-form-7/includes/css/styles.css?ver=1.0.51-24",
    ];

    expect(prioritizeStylesheetUrls(urls, "https://skinsecret.no/")[0]).toBe(
      "https://skinsecret.no/wp-content/themes/skinsecret/style.css?ver=1.0.51-24",
    );
  });

  it("ignores vendor primary variables and keeps site brand variables", () => {
    expect(
      extractCustomPropertyColors(
        `
          :root {
            --wc-primary: #720eec;
            --wc-primary-text: #fcfbfe;
            --color-primary: #F1DCDC;
            --color-secondary: #F8F6F2;
          }
        `,
        cssColorToHex,
      ),
    ).toEqual(["#f1dcdc"]);
  });
});
