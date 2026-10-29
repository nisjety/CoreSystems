import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("./HeroParallax", () => ({ HeroParallax: () => null }));
vi.mock("./SignalPathLayer", () => ({ SignalPathLayer: () => null }));
vi.mock("@/shared/hooks/usePrefersReducedMotion", () => ({
	usePrefersReducedMotion: () => false,
}));

import { HeroSection } from "./HeroSection";

describe("homepage hero promise", () => {
	it("states the governed business-environment category and its operating model", () => {
		const html = renderToStaticMarkup(<HeroSection />);

		expect(html).toContain("En arbeidsflate.");
		expect(html).toContain("For mennesker og KI sammen.");
		expect(html).toContain(
			"Verevon samler virksomhetens systemer, kunnskap og arbeidslogikk i ett felles, styrt miljø. AI arbeider innenfor reglene dere setter, og menneskene tar beslutningene.",
		);
		expect(html).toContain("Systemer");
		expect(html).toContain("Arbeidslogikk");
		expect(html).toContain("Kontroll");
	});

	it("has one primary product action and a distinct contact action", () => {
		const html = renderToStaticMarkup(<HeroSection />);

		expect(html).toContain('href="#flyt"');
		expect(html).toContain("Se Verevon i arbeid");
		expect(html).toContain('href="#kontakt"');
		expect(html).toContain("Snakk med oss");
		expect(html).not.toContain("Be om tidlig tilgang");
	});
});
