import { describe, expect, it } from "vitest";
import { commerceFaqs } from "./commerce-page-data";
import { webSolutionFaqs } from "./web-solutions-page-data";

describe("service answer content", () => {
  it("gives distinct, direct answers for website questions", () => {
    expect(webSolutionFaqs).toHaveLength(2);
    expect(new Set(webSolutionFaqs.map((faq) => faq.question)).size).toBe(2);
    expect(webSolutionFaqs.every((faq) => faq.answer.length > 80)).toBe(true);
  });

  it("gives distinct, direct answers for ecommerce questions", () => {
    expect(commerceFaqs).toHaveLength(2);
    expect(new Set(commerceFaqs.map((faq) => faq.question)).size).toBe(2);
    expect(commerceFaqs.every((faq) => faq.answer.length > 80)).toBe(true);
  });
});
