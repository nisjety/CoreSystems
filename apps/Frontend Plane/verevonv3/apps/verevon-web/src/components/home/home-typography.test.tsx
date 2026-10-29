import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const globalsPath = resolve(process.cwd(), "apps/verevon-web/src/app/globals.css");

describe("homepage marketing typography", () => {
	it("uses a restrained type scale and keeps the hero promise within its visual safe area", () => {
		const stylesheet = readFileSync(globalsPath, "utf8");

		expect(stylesheet).toContain("--verevon-home-display: clamp(2.5rem, 3.42vw, 4rem);");
		expect(stylesheet).toContain("--verevon-home-section: clamp(2.35rem, 3.4vw, 3.5rem);");
		expect(stylesheet).toContain("--verevon-home-meta-tracking: 0.08em;");
		const sectionHeadingRule = stylesheet.match(
			/\.verevon-home \.verevon-home-heading\s*\{([\s\S]*?)\n\}/,
		);
		expect(sectionHeadingRule?.[1]).toContain("font-size: var(--verevon-home-section);");

		const heroHeadingRule = stylesheet.match(
			/\.verevon-home \.verevon-hero-heading\s*\{([\s\S]*?)\n\}/,
		);
		expect(heroHeadingRule?.[1]).toContain("font-size: clamp(2.5rem, 2.9vw, 3.75rem);");
		expect(heroHeadingRule?.[1]).toContain("line-height: 0.94;");

		const problemHeadingRule = stylesheet.match(
			/\.verevon-home \.verevon-problem-heading\s*\{([\s\S]*?)\n\}/,
		);
		expect(problemHeadingRule?.[1]).toContain("font-size: clamp(2.4rem, 2.85vw, 3.5rem);");

		const problemBodyRule = stylesheet.match(
			/\.verevon-home \.verevon-problem-body\s*\{([\s\S]*?)\n\}/,
		);
		expect(problemBodyRule?.[1]).toContain("font-size: clamp(1.03rem, 1.0815vw, 1.15875rem);");
	});

	it("keeps prose on Manrope while metadata remains in the interface face", () => {
		const stylesheet = readFileSync(globalsPath, "utf8");

		for (const role of ["verevon-body-lg", "verevon-body"]) {
			const roleRule = stylesheet.match(new RegExp(`\\.verevon-home \\.${role}\\s*\\{([\\s\\S]*?)\\n\\}`));
			expect(roleRule?.[1]).toContain("font-family: var(--font-manrope), sans-serif;");
		}

		const eyebrowRule = stylesheet.match(/\.verevon-home \.verevon-eyebrow\s*\{([\s\S]*?)\n\}/);
		expect(eyebrowRule?.[1]).toContain("font-family: var(--font-geist), sans-serif;");
	});
});
