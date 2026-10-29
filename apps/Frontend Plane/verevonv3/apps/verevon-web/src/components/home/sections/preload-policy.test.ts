import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const section = (name: string) =>
	readFileSync(resolve(process.cwd(), `apps/verevon-web/src/components/home/sections/${name}`), "utf8");

describe("below-the-fold image loading", () => {
	it("does not preload product-loop images before the visitor reaches the section", () => {
		const source = section("ProductLoopSection.tsx");
		expect(source).not.toContain("priority={index === 0}");
		expect(source).not.toMatch(/media=\{loopStages\[0\]\.media as ProductLoopMedia\}\s+priority/);
	});

	it("does not preload platform-layer art before the visitor reaches the section", () => {
		expect(section("layer-section.tsx")).not.toMatch(/priority=\{\s*layer\.id === "management"\s*\}/);
	});
});
