import { describe, expect, it } from "vitest";
import { contactPageContent } from "./contact-page-content";

describe("contact page content", () => {
  it("keeps the page focused on a concrete first conversation", () => {
    expect(contactPageContent.heading).toBe("KONTAKT");
    expect(contactPageContent.responseTime).toContain("én arbeidsdag");
    expect(contactPageContent.address).toEqual([
      "Sigurd Hoels vei 114",
      "0655 Oslo, Norge",
    ]);
    expect(contactPageContent.image.src).toBe(
      "https://images.unsplash.com/photo-1640006588210-df5dba064198?auto=format&fit=crop&w=1800&q=85",
    );
    expect(contactPageContent.image.alt).toContain("Oslo");
  });
});
