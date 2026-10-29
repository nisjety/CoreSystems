import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { REVEAL_HOLD_MS, REVEAL_SETTLED_MS, ribbonPoint, ribbonTurn, ribbonView } from "./reveal-motion";
import { RIBBON_LENGTH, RIBBON_MESH, RIBBON_THICKNESS, materialPoint, sourcePoint } from "./ribbon-material";
import { reversePoint, reverseTurn } from "./reverse-motion";

describe("B is the original ribbon with only the turn direction reversed", () => {
	it("negates the angle and travelling derivative without retiming any material station", () => {
		for (let time = 0; time <= 2500; time += 20) for (const u of [0, 0.1, 0.5, 0.9, 1]) {
			const original = ribbonTurn(RIBBON_LENGTH * u, time);
			const reversed = reverseTurn(RIBBON_LENGTH * u, time);
			expect(reversed.angle).toBe(-original.angle);
			expect(reversed.gradient).toBe(-original.gradient);
		}
		expect(reverseTurn(RIBBON_LENGTH / 2, 1400).angle).toBeLessThan(0);
	});

	it("reverses the full spatial derivative used by the thickness normal", () => {
		for (const time of [600, 1100, 1600, 2100]) for (const u of [0.1, 0.5, 0.9]) {
			const s = RIBBON_LENGTH * u;
			const derivative = (reverseTurn(s + 0.0001, time).angle - reverseTurn(s - 0.0001, time).angle) / 0.0002;
			expect(reverseTurn(s, time).gradient).toBeCloseTo(derivative, 7);
		}
	});

	it("reflects depth rather than moving or squeezing material fibres", () => {
		for (const time of [0, 600, 900, 1400, 1800, 2100, 2500]) for (const [s, q] of RIBBON_MESH.vertices) {
			const a = ribbonPoint([s, q, 0], time);
			const b = reversePoint([s, q, 0], time);
			b.forEach((v, i) => expect(v).toBeCloseTo([a[0], a[1], -a[2]][i], 10));
			const [along, across] = materialPoint([b[0], b[1]]);
			expect(along).toBeCloseTo(s, 9);
			expect(across ** 2 + b[2] ** 2).toBeCloseTo(q ** 2, 8);
		}
	});

	it("mirrors front, back and edge samples with the same thickness-normal orientation", () => {
		for (const time of [600, 900, 1400, 1800, 2100]) for (let i = 0; i < RIBBON_MESH.vertices.length; i += 9) {
			const [s, q, layer] = RIBBON_MESH.vertices[i];
			const a = ribbonPoint([s, q, -layer], time);
			const b = reversePoint([s, q, layer], time);
			b.forEach((v, axis) => expect(v).toBeCloseTo([a[0], a[1], -a[2]][axis], 10));
		}
	});

	it("keeps the original finite thickness and exact source at both endpoints", () => {
		for (const time of [0, REVEAL_HOLD_MS, REVEAL_SETTLED_MS, 2500]) for (const [s, q] of RIBBON_MESH.vertices) {
			const source = sourcePoint([s, q]);
			const front = ribbonView(reversePoint([s, q, RIBBON_THICKNESS / 2], time), time);
			front.slice(0, 2).forEach((v, i) => expect(v).toBeCloseTo(source[i], 10));
		}
		for (const time of [600, 1000, 1400, 1800, 2200]) for (const [s, q] of RIBBON_MESH.vertices) {
			const front = reversePoint([s, q, RIBBON_THICKNESS / 2], time);
			const back = reversePoint([s, q, -RIBBON_THICKNESS / 2], time);
			expect(Math.hypot(...front.map((v, i) => v - back[i]))).toBeCloseTo(RIBBON_THICKNESS, 10);
		}
	});

	it("keeps the reversed turn inside the same camera and viewBox", () => {
		let bounds = [Infinity, -Infinity, Infinity, -Infinity, 0];
		for (let time = 0; time <= 2500; time += 25) for (const vertex of RIBBON_MESH.vertices) {
			const [x, y, z] = ribbonView(reversePoint(vertex, time), time);
			bounds = [Math.min(bounds[0], x), Math.max(bounds[1], x), Math.min(bounds[2], y), Math.max(bounds[3], y), Math.max(bounds[4], Math.abs(z))];
		}
		expect(bounds[0]).toBeGreaterThan(20);
		expect(bounds[1]).toBeLessThan(102);
		expect(bounds[2]).toBeGreaterThan(26);
		expect(bounds[3]).toBeLessThan(94);
		expect(bounds[4]).toBeLessThan(64);
	});

	it("copies the entire original native renderer with only the two rotation signs and export name changed", () => {
		const read = (name: string) => readFileSync(fileURLToPath(new URL(name, import.meta.url)), "utf8");
		const expected = read("./ribbon-renderer.ts")
			.replace("float theta = 6.28318530718", "float theta = -6.28318530718")
			.replace("float gradient = -6.28318530718", "float gradient = 6.28318530718")
			.replace("export function createRibbonRenderer(", "export function createReverseRenderer(");
		expect(read("./reverse-renderer.ts")).toBe(expected);
	});
});
