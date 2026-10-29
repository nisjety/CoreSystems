import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const sectionSource = (name: string) =>
	readFileSync(resolve(process.cwd(), `apps/verevon-web/src/components/home/sections/${name}.tsx`), "utf8");

describe("homepage narrative", () => {
	it("frames the problem as fragmented business logic before directing visitors to the product", () => {
		const problem = sectionSource("ProblemSection");

		expect(problem).toContain("Arbeidslogikken er deres.");
		expect(problem).toContain("Regler, kunnskap og ansvar er spredt på tvers av systemer");
		expect(problem).toContain("Når ingen ser hele sammenhengen, stopper arbeidet.");
		expect(problem).toContain('href="#flyt"');
		expect(problem).not.toContain("gjør neste steg klart");
	});

	it("uses the feature section to describe one governed business environment", () => {
		const features = sectionSource("FeaturesSection");

		expect(features).toContain("Virksomheten, i én sammenheng.");
		expect(features).toContain("systemer, kunnskap, AI og kontroll henger sammen");
		expect(features).not.toContain("Fra kunnskap til handling i systemene.");
	});

	it("ends on completed work and directs the primary action to the product story", () => {
		const closing = sectionSource("PreFooterStatementSection");

		expect(closing).toContain('["Få", "arbeidet", "gjort."]');
		expect(closing).toContain('href="#flyt"');
		expect(closing).toContain("Se Verevon i arbeid");
		expect(closing).not.toContain('["Få", "neste", "steg", "klart."]');
	});
});
