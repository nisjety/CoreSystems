import { readContour, REST_PATHS, type Point } from "./flag-motion";
import { contourSection } from "./ribbon-strip";

export type Vector3 = readonly [number, number, number];
export const RIBBON_ORIGIN: Point = [74.4, 61.7];
const axisLength = Math.hypot(11.85, -25.7);
export const RIBBON_AXIS: Point = [11.85 / axisLength, -25.7 / axisLength];
export const RIBBON_NORMAL: Point = [-RIBBON_AXIS[1], RIBBON_AXIS[0]];
export const RIBBON_THICKNESS = 0.28;
// A shallow orthographic view makes lifted material pass over the back face.
// The rest plane remains registered exactly to the source SVG.
export const RIBBON_VIEW_SHEAR = 0.5;

export function materialPoint([x, y]: Point): Point {
	const dx = x - RIBBON_ORIGIN[0];
	const dy = y - RIBBON_ORIGIN[1];
	return [dx * RIBBON_AXIS[0] + dy * RIBBON_AXIS[1], dx * RIBBON_NORMAL[0] + dy * RIBBON_NORMAL[1]];
}

export function sourcePoint([s, q]: Point): Point {
	return [RIBBON_ORIGIN[0] + s * RIBBON_AXIS[0] + q * RIBBON_NORMAL[0],
		RIBBON_ORIGIN[1] + s * RIBBON_AXIS[1] + q * RIBBON_NORMAL[1]];
}

const toMaterialCurvePoint = (p: Point): Point => {
	const [s, q] = materialPoint(p);
	return [q, s];
};
const materialCurves = readContour(REST_PATHS[1]).map(([a, b, c, d]) =>
	[toMaterialCurvePoint(a), toMaterialCurvePoint(b), toMaterialCurvePoint(c), toMaterialCurvePoint(d)] as const);
export const RIBBON_LENGTH = Math.max(...materialCurves.flatMap((curve) => curve.map((p) => p[1])));
const stations = [...Array.from({ length: 193 }, (_, i) => RIBBON_LENGTH * i / 192),
	...materialCurves.map((curve) => curve[0][1])]
	.sort((a, b) => a - b).filter((s, i, all) => i === 0 || s - all[i - 1] > 1e-8);

// Source-contour intersections preserve the oblique cap and original curl.
// Shared transverse vertices also keep the curved thickness normal accurate
// inside the surface, not only at its two silhouette edges.
export const RIBBON_COLUMNS = 9;
const rowSize = RIBBON_COLUMNS * 2;
const vertices: readonly Vector3[] = stations.flatMap((s) => {
	const { left, right } = contourSection(materialCurves, s);
	return [1, -1].flatMap((side) => Array.from({ length: RIBBON_COLUMNS }, (_, column): Vector3 =>
		[s, left + (right - left) * column / (RIBBON_COLUMNS - 1), side * RIBBON_THICKNESS / 2]));
});
const quad = (a: number, b: number, c: number, d: number) => [a, b, c, a, c, d];
const rows = stations.slice(0, -1).map((_, i) => i * rowSize);
const columns = Array.from({ length: RIBBON_COLUMNS - 1 }, (_, i) => i);
const end = vertices.length - rowSize;
export const RIBBON_MESH = {
	vertices,
	faces: [
		{ role: "front", indices: rows.flatMap((i) => columns.flatMap((c) => quad(i + c, i + c + 1, i + rowSize + c + 1, i + rowSize + c))) },
		{ role: "back", indices: rows.flatMap((i) => columns.flatMap((c) => quad(i + RIBBON_COLUMNS + c, i + rowSize + RIBBON_COLUMNS + c, i + rowSize + RIBBON_COLUMNS + c + 1, i + RIBBON_COLUMNS + c + 1))) },
		{ role: "edge", indices: [...rows.flatMap((i) => [
			...quad(i, i + rowSize, i + rowSize + RIBBON_COLUMNS, i + RIBBON_COLUMNS),
			...quad(i + RIBBON_COLUMNS - 1, i + rowSize - 1, i + rowSize * 2 - 1, i + rowSize + RIBBON_COLUMNS - 1),
		]), ...columns.flatMap((c) => [
			...quad(c, c + RIBBON_COLUMNS, c + RIBBON_COLUMNS + 1, c + 1),
			...quad(end + c, end + c + 1, end + c + RIBBON_COLUMNS + 1, end + c + RIBBON_COLUMNS),
		])] },
	] as const,
} as const;

const cross = (a: Point, b: Point, c: Point) => (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);

function triangulate(points: readonly Point[]): readonly number[] {
	const signedArea = points.reduce((sum, p, i) => { const next = points[(i + 1) % points.length]; return sum + p[0] * next[1] - p[1] * next[0]; }, 0);
	const sign = Math.sign(signedArea);
	let remaining = points.map((_, i) => i);
	let triangles: readonly number[] = [];
	while (remaining.length > 3) {
		const ear = remaining.findIndex((b, i) => {
			const a = remaining[(i + remaining.length - 1) % remaining.length];
			const c = remaining[(i + 1) % remaining.length];
			if (sign * cross(points[a], points[b], points[c]) <= 1e-9) return false;
			return !remaining.some((p) => p !== a && p !== b && p !== c
				&& sign * cross(points[a], points[b], points[p]) >= -1e-9
				&& sign * cross(points[b], points[c], points[p]) >= -1e-9
				&& sign * cross(points[c], points[a], points[p]) >= -1e-9);
		});
		if (ear < 0) throw new Error("Cannot triangulate Verevon's canonical body");
		triangles = [...triangles, remaining[(ear + remaining.length - 1) % remaining.length], remaining[ear], remaining[(ear + 1) % remaining.length]];
		remaining = remaining.filter((_, i) => i !== ear);
	}
	return [...triangles, ...remaining];
}

const sampledBody = readContour(REST_PATHS[0]).map((curve) => curve[0]);
const uniqueBody = sampledBody.filter((p, i) => i === 0 || Math.hypot(p[0] - sampledBody[i - 1][0], p[1] - sampledBody[i - 1][1]) > 1e-8);
const bodyPoints = Math.hypot(uniqueBody[0][0] - uniqueBody.at(-1)![0], uniqueBody[0][1] - uniqueBody.at(-1)![1]) < 1e-8
	? uniqueBody.slice(0, -1) : uniqueBody;
const bodyBoundary = bodyPoints.filter((p, i) => Math.abs(cross(bodyPoints[(i + bodyPoints.length - 1) % bodyPoints.length], p, bodyPoints[(i + 1) % bodyPoints.length])) > 1e-9);
export const BODY_MESH = {
	vertices: bodyBoundary.map(([x, y]): Vector3 => [x, y, 0]),
	indices: triangulate(bodyBoundary),
} as const;
