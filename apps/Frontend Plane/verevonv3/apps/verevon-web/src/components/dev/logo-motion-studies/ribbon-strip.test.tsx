import { describe, expect, it } from "vitest";
import { ribbonCurves, type RibbonSection } from "./ribbon-strip";

describe("smooth ribbon fitting", () => {
	it("preserves smooth tangents and curvature across unevenly spaced knots", () => {
		const sections: RibbonSection[] = [0, 0.2, 0.201, 0.5, 1].map((along) => ({
			along, inner: [70 + 3 * along ** 2, 36 + 27 * along],
			outer: [84 + 3 * along ** 2, 36 + 27 * along],
		}));
		const curves = ribbonCurves(sections);
		curves.forEach(({ inner, outer }, index) => {
			const span = sections[index + 1].along - sections[index].along;
			for (const curve of [inner, outer]) {
				expect(3 * (curve[1][0] - curve[0][0]) / span).toBeCloseTo(6 * sections[index].along, 7);
				expect(3 * (curve[3][0] - curve[2][0]) / span).toBeCloseTo(6 * sections[index + 1].along, 7);
				expect(6 * (curve[2][0] - 2 * curve[1][0] + curve[0][0]) / span ** 2).toBeCloseTo(6, 6);
			}
		});
	});

	it("keeps two-section straight ribbons exact", () => {
		const sections: RibbonSection[] = [0, 1].map((along) => ({
			along, inner: [70 + along, 36 + 27 * along], outer: [84 + along, 36 + 27 * along],
		}));
		const [curve] = ribbonCurves(sections);
		expect(curve.inner[0]).toEqual(sections[0].inner);
		expect(curve.inner[3]).toEqual(sections[1].inner);
		expect(curve.inner[1][0]).toBeCloseTo(70 + 1 / 3);
		expect(curve.inner[2][0]).toBeCloseTo(70 + 2 / 3);
	});
});
