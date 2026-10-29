import { RIBBON_COLUMNS, RIBBON_MESH, RIBBON_THICKNESS } from "./ribbon-material";

// [canonical s, canonical q, thickness layer, attached s, attached q].
export type ConnectedVertex = readonly [number, number, number, number, number];
export const CONNECTED_FOOT = -8;
export const CONNECTED_HINGE = -4;
export const CONNECTED_SOURCE_JOIN = 3;
export const connectedEase = (value: number) => {
	const t = Math.max(0, Math.min(1, value));
	return t ** 3 * (t * (t * 6 - 15) + 10);
};
export const connectedEaseSlope = (value: number) => {
	const t = Math.max(0, Math.min(1, value));
	return 30 * t ** 2 * (1 - t) ** 2;
};

const collarRows = Array.from({ length: 96 }, (_, i) => CONNECTED_FOOT * (1 - i / 96));
const collar = collarRows.flatMap((s) => {
	const halfWidth = s <= CONNECTED_HINGE
		? 0.7 + (0.4 - 0.7) * connectedEase((s - CONNECTED_FOOT) / 4)
		: 0.4 + (0.55 - 0.4) * connectedEase((s - CONNECTED_HINGE) / 4);
	return [1, -1].flatMap((side) => Array.from({ length: RIBBON_COLUMNS }, (_, column): ConnectedVertex =>
		[0, 0, side * RIBBON_THICKNESS / 2, s, halfWidth * (2 * column / (RIBBON_COLUMNS - 1) - 1)]));
});
const wing = RIBBON_MESH.vertices.map(([s, q, layer], i): ConnectedVertex => {
	const neck = 0.55 * (2 * (i % RIBBON_COLUMNS) / (RIBBON_COLUMNS - 1) - 1);
	const blend = connectedEase(s / CONNECTED_SOURCE_JOIN);
	return [s, q, layer, s, neck + (q - neck) * blend];
});
const vertices = [...collar, ...wing];
const size = RIBBON_COLUMNS * 2;
const rows = Array.from({ length: vertices.length / size - 1 }, (_, i) => i * size);
const columns = Array.from({ length: RIBBON_COLUMNS - 1 }, (_, i) => i);
const quad = (a: number, b: number, c: number, d: number) => [a, b, c, a, c, d];
const end = vertices.length - size;

// One indexed mesh, with shared rows from the buried foot to the top cap.
// There is no separate connector object, opacity layer, or independent tip.
export const CONNECTED_RIBBON_MESH = {
	vertices,
	faces: [
		{ role: "front", indices: rows.flatMap((i) => columns.flatMap((c) => quad(i + c, i + c + 1, i + size + c + 1, i + size + c))) },
		{ role: "back", indices: rows.flatMap((i) => columns.flatMap((c) => quad(i + RIBBON_COLUMNS + c, i + size + RIBBON_COLUMNS + c, i + size + RIBBON_COLUMNS + c + 1, i + RIBBON_COLUMNS + c + 1))) },
		{ role: "edge", indices: [...rows.flatMap((i) => [
			...quad(i, i + size, i + size + RIBBON_COLUMNS, i + RIBBON_COLUMNS),
			...quad(i + RIBBON_COLUMNS - 1, i + size - 1, i + size * 2 - 1, i + size + RIBBON_COLUMNS - 1),
		]), ...columns.flatMap((c) => [
			...quad(c, c + RIBBON_COLUMNS, c + RIBBON_COLUMNS + 1, c + 1),
			...quad(end + c, end + c + 1, end + c + RIBBON_COLUMNS + 1, end + c + RIBBON_COLUMNS),
		])] },
	] as const,
} as const;
