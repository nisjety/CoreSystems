import { describe, expect, it } from "vitest";
import { RIBBON_LENGTH, RIBBON_THICKNESS, sourcePoint, materialPoint, BODY_MESH } from "./ribbon-material";
import { ribbonTurn } from "./reveal-motion";
import { CONNECTED_RIBBON_MESH } from "./connected-material";
import { attachmentAmount, connectedTurn, connectedPoint, connectedView, CONNECTED_SETTLED_MS } from "./connected-motion";

describe("connected reverse ribbon B", () => {
	it("closes before rolling and restores the original gap only during settling", () => {
		expect(attachmentAmount(0)).toBe(0);
		for (const time of [200, 260, 800, 1400, 2000, 2070]) expect(attachmentAmount(time)).toBe(1);
		expect(attachmentAmount(2200)).toBeGreaterThan(attachmentAmount(2300));
		expect(attachmentAmount(CONNECTED_SETTLED_MS)).toBe(0);
		expect(CONNECTED_SETTLED_MS).toBeLessThanOrEqual(2500);
	});
	it("rotates in the opposite direction, with no abrupt derivative at the attachment", () => {
		for (const time of [800, 1100, 1400, 1700]) {
			expect(connectedTurn(RIBBON_LENGTH * 0.5, time).angle).toBeLessThan(0);
			expect(ribbonTurn(RIBBON_LENGTH * 0.5, time).angle).toBeGreaterThan(0);
			expect(connectedTurn(-8, time).angle).toBeCloseTo(0, 10);
			expect(connectedTurn(-8, time).gradient).toBeCloseTo(0, 10);
			for (const s of [-3.5, -2, -0.5, 0, 1, 12, 25]) {
				const derivative = (connectedTurn(s + 1e-4, time).angle - connectedTurn(s - 1e-4, time).angle) / 2e-4;
				expect(connectedTurn(s, time).gradient).toBeCloseTo(derivative, 6);
			}
		}
		const velocity = (fn: typeof connectedTurn, time: number) => Math.abs(fn(0, time + 0.1).angle - fn(0, time - 0.1).angle) / 0.2;
		// Same easing family, extended turn: lower peak speed than saved A.
		expect(velocity(connectedTurn, 1145)).toBeLessThan(velocity(ribbonTurn, 1020));
	});
	it("keeps a finite flat foot embedded in the stationary body throughout the roll", () => {
		const foot = CONNECTED_RIBBON_MESH.vertices.filter((v) => v[3] === -8 && v[2] > 0);
		expect(foot.length).toBeGreaterThan(2);
		const insideBody = ([x, y]: readonly number[]) => {
			let inside = false;
			BODY_MESH.vertices.forEach((a, i) => {
				const b = BODY_MESH.vertices[(i + 1) % BODY_MESH.vertices.length];
				if ((a[1] > y) !== (b[1] > y) && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0]) inside = !inside;
			});
			return inside;
		};
		for (let time = 260; time <= 2070; time += 25) for (const v of foot) {
			const p = connectedPoint(v, time);
			expect(p).toEqual(connectedPoint(v, 260));
			expect(p[2]).toBeCloseTo(0, 10);
			expect(insideBody(p)).toBe(true);
		}
	});
	it("does not squeeze fibres or change thickness during the active roll", () => {
		for (const time of [400, 900, 1400, 1900]) for (const v of CONNECTED_RIBBON_MESH.vertices.filter((_, i) => i % 7 === 0)) {
			const p = connectedPoint([v[0], v[1], 0, v[3], v[4]], time);
			const [s, q] = materialPoint([p[0], p[1]]);
			expect(s).toBeCloseTo(v[3], 8);
			expect(q * q + (p[2] + RIBBON_THICKNESS / 2) ** 2).toBeCloseTo(v[4] ** 2, 8);
			const a = connectedPoint([v[0], v[1], RIBBON_THICKNESS / 2, v[3], v[4]], time);
			const b = connectedPoint([v[0], v[1], -RIBBON_THICKNESS / 2, v[3], v[4]], time);
			expect(Math.hypot(...a.map((n, i) => n - b[i]))).toBeCloseTo(RIBBON_THICKNESS, 9);
		}
	});
	it("retracts the entire collar, including thickness, with no leftover bead at the gap", () => {
		for (const v of CONNECTED_RIBBON_MESH.vertices) {
			const p = connectedView(connectedPoint(v, 2400), 2400);
			const original = sourcePoint([v[0], v[1]]);
			expect(p[0]).toBeCloseTo(original[0], 9);
			expect(p[1]).toBeCloseTo(original[1], 9);
			if (v[3] < 0) p.forEach((n, i) => expect(n).toBeCloseTo([74.4, 61.7, 0][i], 10));
		}
	});
	it("is one closed indexed ribbon, including the attachment—not a second connector draw", () => {
		const { vertices, faces } = CONNECTED_RIBBON_MESH;
		const edges = faces.flatMap(({ indices }) => indices.flatMap((_, i) => i % 3 ? [] : [
			[indices[i], indices[i + 1]], [indices[i + 1], indices[i + 2]], [indices[i + 2], indices[i]],
		]));
		const counts = new Map<string, number>();
		for (const [a, b] of edges) {
			expect(Math.max(a, b)).toBeLessThan(vertices.length);
			const key = [a, b].sort((x, y) => x - y).join(":");
			counts.set(key, (counts.get(key) ?? 0) + 1);
		}
		expect([...counts.values()].every((count) => count === 2)).toBe(true);
		const seen = new Set([0]);
		for (let pass = 0; pass < vertices.length && seen.size < vertices.length; pass++) for (const [a, b] of edges) {
			if (seen.has(a)) seen.add(b);
			if (seen.has(b)) seen.add(a);
		}
		expect(seen.size).toBe(vertices.length);
	});
	it("keeps every transformed vertex finite and inside the existing camera bounds", () => {
		let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity, invalid = 0;
		for (let time = 0; time <= 2500; time += 10) for (const vertex of CONNECTED_RIBBON_MESH.vertices) {
			const p = connectedView(connectedPoint(vertex, time), time);
			minX = Math.min(minX, p[0]); maxX = Math.max(maxX, p[0]);
			minY = Math.min(minY, p[1]); maxY = Math.max(maxY, p[1]);
			if (!p.every(Number.isFinite)) invalid++;
		}
		expect(invalid).toBe(0);
		expect(minX).toBeGreaterThan(20); expect(maxX).toBeLessThan(102);
		expect(minY).toBeGreaterThan(26); expect(maxY).toBeLessThan(94);
	});
});
