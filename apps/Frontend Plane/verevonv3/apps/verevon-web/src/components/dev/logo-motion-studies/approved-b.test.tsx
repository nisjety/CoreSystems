import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

describe("approved B is preserved as the original", () => {
	for (const [file, hash] of [
		["reverse-motion.ts", "d55b108b582038d2d3b6f239edce46ec504be086237ce5560fef85387921fb56"],
		["reverse-renderer.ts", "def2fa3ab5edfad4474ab16b6b3b687dbf3ee77317d09f4b84a1f6fb2829ca70"],
		["RibbonComparison.tsx", "19d6e6b6a22b763727f22fafe25170e876e1505e6692fc703a135ebc0498880a"],
	]) it(`keeps ${file} byte-for-byte unchanged`, () => {
		const source = readFileSync(fileURLToPath(new URL(file, import.meta.url)));
		expect(createHash("sha256").update(source).digest("hex")).toBe(hash);
	});
});
