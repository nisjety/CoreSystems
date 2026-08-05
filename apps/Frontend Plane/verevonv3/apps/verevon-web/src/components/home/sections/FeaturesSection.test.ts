import { describe, expect, it } from "vitest";
import { platformCards, workflowCards } from "./FeatureWorkflowCards";

describe("features platform cards", () => {
	it("uses the six shared platform surfaces", () => {
		expect(workflowCards).toHaveLength(4);
		expect(platformCards).toHaveLength(6);
		expect(platformCards.map((card) => card.film)).toEqual([
			"build",
			"connect",
			"ground",
			"research",
			"actions",
			"approve",
		]);
	});
});
