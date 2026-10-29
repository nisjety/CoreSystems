import { describe, expect, it } from "vitest";
import { buildContactEmail } from "./email-template";

const submission = {
  name: "Kari Nordmann",
  email: "kari@bedrift.no",
  company: "Bedrift AS",
  projectType: "app" as const,
  message: "Vi trenger en app.\nGjerne før jul.",
  submissionId: "95572209-f995-45a3-93b8-b6e8f115b156",
  source: "direct" as const,
  website: "" as const,
};

describe("contact email template", () => {
  it("uses readable labels and a descriptive subject", () => {
    const email = buildContactEmail(submission);
    expect(email.subject).toBe("Ny henvendelse: Kari Nordmann – Produkt eller app");
    expect(email.text).toContain("Gjelder: Produkt eller app");
    expect(email.text).toContain("Kilde:   Kontaktsiden");
    expect(email.html).toContain("Vi trenger en app.<br>Gjerne før jul.");
  });

  it("escapes visitor input in the HTML version", () => {
    const email = buildContactEmail({
      ...submission,
      name: "<script>alert(1)</script>",
      message: '<img src=x onerror="x">',
    });
    expect(email.html).not.toContain("<script>");
    expect(email.html).not.toContain("<img src=x");
    expect(email.html).toContain("&lt;script&gt;");
  });

  it("marks a missing company as not provided", () => {
    const email = buildContactEmail({ ...submission, company: "" });
    expect(email.text).toContain("Bedrift: Ikke oppgitt");
  });

  it("flags enquiries from the English site so the reply is in English", () => {
    expect(buildContactEmail(submission).text).toContain("Språk:   Norsk");
    const email = buildContactEmail({ ...submission, locale: "en" });
    expect(email.text).toContain("Språk:   Engelsk – svar på engelsk");
    expect(email.html).toContain("Engelsk – svar på engelsk");
  });
});
