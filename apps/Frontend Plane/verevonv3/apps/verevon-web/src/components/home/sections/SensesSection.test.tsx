import { describe, expect, it, vi } from "vitest";

vi.hoisted(() => {
	Object.defineProperty(window, "matchMedia", {
		configurable: true,
		value: () => ({
			addEventListener() {},
			addListener() {},
			dispatchEvent: () => false,
			matches: false,
			media: "",
			onchange: null,
			removeEventListener() {},
			removeListener() {},
		}),
	});
});
import { chapters } from "./SensesSection";

describe("SensesSection chapter copy", () => {
	it("explains shared context, governed progress, and human decisions", () => {
		expect(chapters).toHaveLength(3);
		expect(chapters[0].text).toContain("Arbeid som følger reglene deres kan gå videre");
		expect(chapters[1].text).toContain("konteksten virksomheten allerede har");
		expect(chapters[2].titleLines).toEqual(["Behold oversikten.", "Grip inn ved behov."]);
		expect(chapters[2].text).toContain("Følg hva AI bruker som grunnlag");
		expect(chapters[2].text).toContain("riktig person");
	});
});
