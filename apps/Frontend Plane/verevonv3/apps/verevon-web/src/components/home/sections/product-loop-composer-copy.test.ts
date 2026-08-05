import { describe, expect, it } from "vitest";
import { getProductLoopComposerCopy } from "./product-loop-composer-copy";

describe("product loop composer copy", () => {
	it.each([
		["chat", "Spør. Få arbeidet i gang."],
		["crawl", "Hent innhold. Bygg kunnskap."],
		["search", "Søk på tvers. Se grunnlaget."],
	] as const)("returns mode-specific copy for %s", (mode, title) => {
		const copy = getProductLoopComposerCopy(mode);

		expect(copy.title).toBe(title);
		expect(copy.body.length).toBeGreaterThan(40);
		expect(copy.label).toContain("04 /");
	});
});
