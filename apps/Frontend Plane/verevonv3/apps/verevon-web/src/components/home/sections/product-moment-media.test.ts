import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { PRODUCT_MOMENT_MEDIA } from "./product-moment-media";

const publicDirectories = [
  resolve(process.cwd(), "public"),
  resolve(process.cwd(), "apps/verevon-web/public"),
];
const hasPublicAsset = (assetPath: string) =>
  publicDirectories.some((directory) =>
    existsSync(resolve(directory, assetPath.replace(/^\//, ""))),
  );

describe("product moment media", () => {
	it("maps the three product actions to distinct local video and poster assets", () => {
		const moments = Object.values(PRODUCT_MOMENT_MEDIA);

		expect(moments).toHaveLength(3);
		expect(new Set(moments.map((moment) => moment.src)).size).toBe(3);
		expect(new Set(moments.map((moment) => moment.poster)).size).toBe(3);

		for (const moment of moments) {
			expect(moment.kind).toBe("video");
			expect(moment.src).toMatch(/^\/verevon-product-moments\/.+\.mp4$/);
			expect(moment.poster).toMatch(/^\/verevon-product-moments\/.+\.jpg$/);
			expect(moment.alt.trim().length).toBeGreaterThan(24);
    expect(hasPublicAsset(moment.src)).toBe(true);
    expect(hasPublicAsset(moment.poster)).toBe(true);
		}
	});
});
