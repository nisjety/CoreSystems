import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { siteDescription } from "@/shared/seo/site";

const layoutPath = resolve(process.cwd(), "apps/verevon-web/src/app/layout.tsx");
const homePagePath = resolve(process.cwd(), "apps/verevon-web/src/app/page.tsx");

describe("RootLayout", () => {
	it("suppresses hydration warnings for the intentional pre-hydration homepage session marker", () => {
		const layout = readFileSync(layoutPath, "utf8");

		expect(layout).toContain(
			'<html data-motion="full" data-scroll-behavior="smooth" lang="nb" suppressHydrationWarning>',
		);
	});

	it("uses the launch narrative in search and structured application metadata", () => {
		const layout = readFileSync(layoutPath, "utf8");

		expect(siteDescription).toContain("handler innenfor avtalte rammer");
		expect(siteDescription).toContain("Mennesker tar beslutningene som betyr noe");
		expect(layout).toContain('"Risikobasert godkjenning"');
		expect(layout).toContain('"Sporbar utførelse"');
		expect(layout).not.toContain('"Godkjenning før handling"');
	});

	it("keeps the homepage canonical and Open Graph URL off other routes", () => {
		const layout = readFileSync(layoutPath, "utf8");
		const homePage = readFileSync(homePagePath, "utf8");

		expect(layout).not.toContain('canonical: "/"');
		expect(layout).not.toContain('url: "/"');
		expect(homePage).toContain('canonical: "/"');
		expect(homePage).toContain('url: "/"');
	});
});
