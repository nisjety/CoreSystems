import { describe, expect, it } from "vitest";
import { connectedHandoff, connectedTurn } from "./connected-motion";
import { RIBBON_LENGTH } from "./ribbon-material";

describe("B's near-rest raster handoff", () => {
	it("keeps the moving ribbon fully opaque through the roll and gap return", () => {
		for (const time of [0, 260, 900, 1400, 2030, 2300, 2350, 2380]) expect(connectedHandoff(time)).toBe(0);
	});
	it("smooths raster antialiasing only after every free wing station has completed its turn", () => {
		for (const s of [-4, 0, RIBBON_LENGTH * 0.5, RIBBON_LENGTH]) expect(connectedTurn(s, 2380).angle).toBeCloseTo(-2 * Math.PI, 8);
		expect(connectedHandoff(2390)).toBeCloseTo(0.5);
		expect(connectedHandoff(2399.99)).toBeGreaterThan(0.999999);
		expect(connectedHandoff(2400)).toBe(1);
	});
});
