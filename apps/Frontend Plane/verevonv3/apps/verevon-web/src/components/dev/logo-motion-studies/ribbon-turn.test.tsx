import { describe, expect, it } from "vitest";
import { readContour, REST_PATHS } from "./flag-motion";
import { REVEAL_DURATION_MS, REVEAL_HOLD_MS, REVEAL_SETTLED_MS, TURN_TRAVEL_MS, TURN_DURATION_MS, TURN_TOP_EXTRA_MS, ribbonPoint, ribbonTurn, ribbonView } from "./reveal-motion";
import { RIBBON_COLUMNS, RIBBON_LENGTH, RIBBON_THICKNESS, RIBBON_MESH, materialPoint, sourcePoint, BODY_MESH } from "./ribbon-material";

describe("depth-turned canonical ribbon", () => {
	it("uses immutable approved material, including its oblique cap", () => {
		for (const curve of readContour(REST_PATHS[1])) for (const p of curve) {
			const restored = sourcePoint(materialPoint(p));
			expect(restored[0]).toBeCloseTo(p[0], 10);
			expect(restored[1]).toBeCloseTo(p[1], 10);
		}
		expect(materialPoint([93.6, 36])[0]).toBeCloseTo(RIBBON_LENGTH, 8);
		expect(materialPoint([78.9, 36])[0]).toBeLessThan(RIBBON_LENGTH - 5);
	});
	it("rotates without shrinking fibres or changing longitudinal position", () => {
		for (let time = 0; time <= 2500; time += 50) for (const [s, q] of RIBBON_MESH.vertices) {
			const [x, y, z] = ribbonPoint([s, q, 0], time);
			const [along, across] = materialPoint([x, y]);
			expect(along).toBeCloseTo(s, 9);
			expect(across * across + z * z).toBeCloseTo(q * q, 8);
		}
	});
	it("reverses edges, shows its back, and turns more slowly at the top", () => {
		const s = RIBBON_LENGTH * 0.5;
		const mid = REVEAL_HOLD_MS + TURN_TRAVEL_MS * 0.5 + (TURN_DURATION_MS + TURN_TOP_EXTRA_MS * 0.5) / 2;
		expect(ribbonTurn(s, mid).angle).toBeCloseTo(Math.PI);
		expect(ribbonPoint([s, 3, 0], mid)[0]).toBeLessThan(ribbonPoint([s, -3, 0], mid)[0]);
		expect(ribbonTurn(RIBBON_LENGTH, 1600).angle).toBeLessThan(ribbonTurn(0, 1600).angle);
		for (let time = 0; time <= 2500; time += 20) {
			// One broad travelling turn, not two simultaneous pinch points.
			expect(ribbonTurn(0, time).angle - ribbonTurn(RIBBON_LENGTH, time).angle).toBeLessThan(Math.PI);
		}
	});
	it("uses the full travelling-twist derivative for its surface normal", () => {
		for (const time of [600, 1100, 1600, 2100]) for (const u of [0.1, 0.5, 0.9]) {
			const s = RIBBON_LENGTH * u;
			const derivative = (ribbonTurn(s + 0.0001, time).angle - ribbonTurn(s - 0.0001, time).angle) / 0.0002;
			expect(ribbonTurn(s, time).gradient).toBeCloseTo(derivative, 7);
		}
	});
	it("has constant thickness and an anchored heel, even edge-on", () => {
		for (let time = 0; time <= 2500; time += 100) {
			ribbonPoint([0, 0, 0], time).forEach((v, i) => expect(v).toBeCloseTo([74.4, 61.7, 0][i], 10));
			for (const [s, q] of RIBBON_MESH.vertices) {
				const a = ribbonPoint([s, q, RIBBON_THICKNESS / 2], time);
				const b = ribbonPoint([s, q, -RIBBON_THICKNESS / 2], time);
				expect(Math.hypot(...a.map((v, i) => v - b[i]))).toBeCloseTo(RIBBON_THICKNESS, 10);
				expect(a.every(Number.isFinite)).toBe(true);
			}
		}
	});
	it("shares vertices across front, back and edge faces in one watertight mesh", () => {
		const { vertices, faces } = RIBBON_MESH;
		const edges = faces.flatMap(({ indices }) => indices.flatMap((_, i) => i % 3 ? [] : [
			[indices[i], indices[i + 1]], [indices[i + 1], indices[i + 2]], [indices[i + 2], indices[i]],
		]));
		const counts = new Map<string, number>();
		for (const [a, b] of edges) {
			expect(a).toBeGreaterThanOrEqual(0);
			expect(b).toBeLessThan(vertices.length);
			const key = [a, b].sort((x, y) => x - y).join(":");
			counts.set(key, (counts.get(key) ?? 0) + 1);
		}
		expect([...counts.values()].every((count) => count === 2)).toBe(true);
		const seen = new Set([0]);
		for (let pass = 0; pass < vertices.length && seen.size < vertices.length; pass++) {
			for (const [a, b] of edges) {
				if (seen.has(a)) seen.add(b);
				if (seen.has(b)) seen.add(a);
			}
		}
		expect(seen.size).toBe(vertices.length);
	});
	it("triangulates the concave body without filling its original crease", () => {
		const { vertices, indices } = BODY_MESH;
		expect(vertices.some(([x, y]) => x === 28 && y === 36)).toBe(true);
		expect(vertices.some(([x, y]) => x === 42.7 && y === 36)).toBe(true);
		const area = vertices.reduce((sum, p, i) => { const next = vertices[(i + 1) % vertices.length]; return sum + p[0] * next[1] - p[1] * next[0]; }, 0) / 2;
		const triangles = indices.reduce((sum, _, i) => {
			if (i % 3) return sum;
			const [a, b, c] = indices.slice(i, i + 3).map((index) => vertices[index]);
			return sum + Math.abs((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) / 2;
		}, 0);
		expect(triangles).toBeCloseTo(Math.abs(area), 8);
	});
	it("lands on exact material coordinates before the 2.5-second deadline", () => {
		expect(REVEAL_DURATION_MS).toBe(2500);
		expect(REVEAL_SETTLED_MS).toBeLessThan(REVEAL_DURATION_MS);
		for (const time of [0, REVEAL_SETTLED_MS, 2500, 100000]) for (const [s, q] of RIBBON_MESH.vertices) {
			const p = ribbonPoint([s, q, 0], time);
			const original = sourcePoint([s, q]);
			expect(p[0]).toBeCloseTo(original[0], 10);
			expect(p[1]).toBeCloseTo(original[1], 10);
			expect(p[2]).toBeCloseTo(0, 10);
			const front = ribbonView(ribbonPoint([s, q, RIBBON_THICKNESS / 2], time), time);
			expect(front[0]).toBeCloseTo(original[0], 10);
			expect(front[1]).toBeCloseTo(original[1], 10);
			const back = ribbonView(ribbonPoint([s, q, -RIBBON_THICKNESS / 2], time), time);
			expect(back[0]).toBeCloseTo(original[0], 10);
			expect(back[1]).toBeCloseTo(original[1], 10);
		}
	});
	it("keeps the entire finite-thickness roll inside the existing SVG viewBox", () => {
		let limits = { left: Infinity, right: -Infinity, top: Infinity, bottom: -Infinity, depth: 0 };
		for (let time = 0; time <= 2500; time += 25) for (const vertex of RIBBON_MESH.vertices) {
			const [x, y, depth] = ribbonView(ribbonPoint(vertex, time), time);
			limits = { left: Math.min(limits.left, x), right: Math.max(limits.right, x),
				top: Math.min(limits.top, y), bottom: Math.max(limits.bottom, y), depth: Math.max(limits.depth, Math.abs(depth)) };
		}
		expect(limits.left).toBeGreaterThan(20);
		expect(limits.right).toBeLessThan(102);
		expect(limits.top).toBeGreaterThan(26);
		expect(limits.bottom).toBeLessThan(94);
		expect(limits.depth).toBeLessThan(64);
	});
	it("projects different parts of the actual surface over one another at different depths", () => {
		const points = RIBBON_MESH.vertices.map(([s, q]) => ribbonView(ribbonPoint([s, q, 0], 1500), 1500));
		const indices = RIBBON_MESH.faces[0].indices;
		const triangles = indices.flatMap((_, i) => i % 3 ? [] : [indices.slice(i, i + 3)]);
		let overlaps = 0;
		for (let s = 0; s < RIBBON_LENGTH; s += 1) for (let q = -9; q <= 9; q += 1) {
			const [x, y] = sourcePoint([s, q]);
			const hits = triangles.flatMap((ids) => {
				const [a, b, c] = ids.map((id) => points[id]);
				const denominator = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
				if (Math.abs(denominator) < 1e-8) return [];
				const u = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / denominator;
				const v = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / denominator;
				const w = 1 - u - v;
				if (Math.min(u, v, w) <= 1e-8) return [];
				return [{ depth: u * a[2] + v * b[2] + w * c[2], facing: Math.sign(denominator) }];
			});
			if (hits.some((a) => hits.some((b) => a.facing !== b.facing && Math.abs(a.depth - b.depth) > 0.1))) overlaps++;
		}
		expect(overlaps).toBeGreaterThan(0);
	});
	it("keeps interpolated interior thickness within 1% of the analytic surface", () => {
		for (const time of [800, 1300, 1700]) for (let i = 0; i < RIBBON_MESH.vertices.length; i += RIBBON_COLUMNS * 2) {
			for (let c = 0; c < RIBBON_COLUMNS - 1; c++) {
				const [a, b, backA, backB] = [i + c, i + c + 1, i + c + RIBBON_COLUMNS, i + c + RIBBON_COLUMNS + 1]
					.map((id) => ribbonPoint(RIBBON_MESH.vertices[id], time));
				const thickness = Math.hypot(...a.map((v, axis) => (v + b[axis] - backA[axis] - backB[axis]) / 2));
				expect(thickness).toBeGreaterThan(RIBBON_THICKNESS * 0.99);
			}
		}
	});
});
