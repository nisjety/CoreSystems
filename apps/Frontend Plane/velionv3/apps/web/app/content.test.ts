import { describe, expect, it } from "vitest";
import { siteConfig, trustPillars, workflows } from "./content";

describe("Velion landing content", () => {
  it("keeps the hero focused on AI customer experience", () => {
    expect(siteConfig.heroTitle).toBe(
      "The AI teammate for customer experience",
    );
    expect(siteConfig.heroBody).toContain("answers customers");
    expect(siteConfig.heroBody).toContain("executes approved actions");
  });

  it("keeps approval before execution in the action workflow", () => {
    const actionWorkflow = workflows.find(
      (workflow) => workflow.title === "Approve actions",
    );

    expect(actionWorkflow?.steps).toEqual([
      "Propose action",
      "Review and approve",
      "Execute safely",
      "Audit log",
    ]);
  });

  it("does not use fabricated trust metrics", () => {
    const trustCopy = trustPillars
      .flatMap((pillar) => [pillar.title, pillar.body])
      .join(" ");

    expect(trustCopy).not.toMatch(/\b\d{2,3}%\b/);
    expect(trustCopy).not.toMatch(/\b\d+x\b/i);
  });
});
