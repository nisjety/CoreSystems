import { describe, expect, it } from "vitest";
import {
  webSolutionCriteria,
  webSolutionProcess,
} from "./web-solutions-page-data";

describe("web solutions service content", () => {
  it("covers the full website delivery process", () => {
    expect(webSolutionProcess.map((step) => step.title)).toEqual([
      "Innsikt",
      "Plan og struktur",
      "Design og innhold",
      "Utvikling",
      "Testing og lansering",
      "Videreutvikling",
    ]);
  });

  it("keeps the three website quality criteria distinct", () => {
    expect(webSolutionCriteria).toHaveLength(3);
    expect(
      new Set(webSolutionCriteria.map((criterion) => criterion.title)).size,
    ).toBe(3);
  });
});
