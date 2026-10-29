import { describe, expect, it } from "vitest";
import { getProductLoopComposerCopy } from "./product-loop-composer-copy";

describe("product loop composer copy", () => {
	it.each([
		["chat", "Spør. Se sammenhengen.", "04 / Spør"],
		["crawl", "Hent innhold. Bygg felles grunnlag.", "04 / Hent"],
		["search", "Søk på tvers. Se grunnlaget.", "04 / Søk"],
	] as const)("returns mode-specific copy for %s", (mode, title, label) => {
		const copy = getProductLoopComposerCopy(mode);

		expect(copy.title).toBe(title);
		expect(copy.body.length).toBeGreaterThan(40);
		expect(copy.label).toBe(label);
	});

	it("puts shared context before a business decision", () => {
		expect(getProductLoopComposerCopy("chat").body).toBe(
			"Beskriv saken. Verevon samler relevant kunnskap, regler og systemdata, slik at dere kan ta neste beslutning på et synlig grunnlag.",
		);
	});
});
