import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

// A is a saved comparison baseline, not a new interpretation of its motion.
const ORIGINAL_HASHES = {
	"flag-motion.ts": "95baca515afdfa21e3cc08f4c1693f2b4ebf615685c9aff31f431740113ded90",
	"ribbon-strip.ts": "3de42fff46b3171c18eb2efb1e002fde5bd2ee853bbae455be0db507885149af",
	"ribbon-material.ts": "8dc184df465e4260e968fe1059a8dd97e5b8246c0a1f000e2fe2a4b806289921",
	"reveal-motion.ts": "b041417fb90a69e70d2839fda9a5e70517806210e80e7fee7424873d77ce69df",
	"ribbon-renderer.ts": "7ef4a99559f791b604d8dae8023f7b97120b9e4f7cb6885f7c1ee508a3c1fc13",
} as const;

describe("saved original variant A", () => {
	for (const [file, hash] of Object.entries(ORIGINAL_HASHES)) it(`preserves ${file} byte for byte`, () => {
		expect(createHash("sha256").update(readFileSync(join(dirname(fileURLToPath(import.meta.url)), file))).digest("hex")).toBe(hash);
	});
});
