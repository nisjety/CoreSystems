import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { PreFooterStatementSection } from "./PreFooterStatementSection";

describe("homepage closing statement", () => {
	it("ends on the shared, governed-workspace promise", () => {
		const html = renderToStaticMarkup(<PreFooterStatementSection />);

		expect(html).toContain("Felles grunnlag.");
		expect(html).toContain("Styrt arbeid.");
		expect(html).toContain("Se sammenhengen.");
		expect(html).toContain("Ta neste steg.");
	});

	it("keeps the product and contact paths clear", () => {
		const html = renderToStaticMarkup(<PreFooterStatementSection />);

		expect(html).toContain('href="#flyt"');
		expect(html).toContain("Se Verevon i arbeid");
		expect(html).toContain('href="#kontakt"');
		expect(html).toContain("Snakk med oss");
	});
});
