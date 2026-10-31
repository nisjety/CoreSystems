import {
	VEREVON_MARK_LEFT_V_PATH,
	VEREVON_MARK_RIGHT_WING_PATH,
} from "@/components/home/sections/VerevonMark";

export type Point = readonly [number, number];
type Curve = readonly [Point, Point, Point, Point];
export type FlagVariant = "breeze" | "flag" | "gust";

export const FLAG_STUDIES = [
	{ id: "breeze", title: "Myk vending", description: "En bred, rolig vending. Kurven blir kanttynn før den åpner seg på motsatt side." },
	{ id: "flag", title: "Rullende fold", description: "En smal vridning vandrer gjennom kurven. Kantene bytter side som et bånd som ruller rundt." },
	{ id: "gust", title: "Lang vridning", description: "En tydeligere forskyvning mellom delene. Den tynne kanten reiser lenger gjennom V-en." },
] as const;

const MOTION = {
	breeze: { start: 0.08, travel: 0.06, duration: 0.78 },
	flag: { start: 0.08, travel: 0.14, duration: 0.70 },
	gust: { start: 0.06, travel: 0.20, duration: 0.66 },
} as const;

export const FLAG_DURATION_MS = 2400;
export const FLAG_LOOP_MS = FLAG_DURATION_MS + 1000;
export const REST_PATHS = [VEREVON_MARK_LEFT_V_PATH, VEREVON_MARK_RIGHT_WING_PATH] as const;

const mix = (a: Point, b: Point, t: number): Point => [
	a[0] + (b[0] - a[0]) * t,
	a[1] + (b[1] - a[1]) * t,
];

function subdivide([a, b, c, d]: Curve): readonly Curve[] {
	const ab = mix(a, b, 0.5);
	const bc = mix(b, c, 0.5);
	const cd = mix(c, d, 0.5);
	const abc = mix(ab, bc, 0.5);
	const bcd = mix(bc, cd, 0.5);
	const center = mix(abc, bcd, 0.5);
	return [[a, ab, abc, center], [center, bcd, cd, d]];
}

// Normalize the existing logo paths, including straight edges, to short cubics.
// Short cubics keep the travelling, edge-on pinch smooth at every frame.
export function readContour(source: string) {
	let cursor: Point = [0, 0];
	let start: Point = cursor;
	return (source.match(/[MLHCZ][^MLHCZ]*/g) ?? []).flatMap((command): Curve[] => {
		const numbers = (command.slice(1).match(/-?\d+(?:\.\d+)?/g) ?? []).map(Number);
		if (command[0] === "M") {
			cursor = [numbers[0], numbers[1]];
			start = cursor;
			return [];
		}
		const end: Point = command[0] === "Z" ? start
			: command[0] === "H" ? [numbers[0], cursor[1]]
			: command[0] === "C" ? [numbers[4], numbers[5]]
			: [numbers[0], numbers[1]];
		const curve: Curve = command[0] === "C"
			? [cursor, [numbers[0], numbers[1]], [numbers[2], numbers[3]], end]
			: [cursor, mix(cursor, end, 1 / 3), mix(cursor, end, 2 / 3), end];
		cursor = end;
		return subdivide(curve).flatMap(subdivide).flatMap(subdivide);
	});
}

const CONTOURS = REST_PATHS.map(readContour);

function smoothstep(from: number, to: number, value: number) {
	const t = Math.max(0, Math.min(1, (value - from) / (to - from)));
	return t * t * (3 - 2 * t);
}

export function flagPoint([x, y]: Point, progress: number, variant: FlagVariant): Point {
	if (progress <= 0 || progress >= 1) return [x, y];
	const { start, travel, duration } = MOTION[variant];
	// Treat the curved right stroke as ONE ribbon, with the left V leg rigid.
	// Every horizontal material section shares an angle: no cross-width mask
	// that could create extra little folds inside the same section.
	const along = (61.7 - y) / 25.7;
	const hinge = 74.4 + 11.85 * along;
	const delay = start + travel * smoothstep(0, 1, along);
	const angle = 2 * Math.PI * smoothstep(delay, delay + duration, progress);
	// The original curve is offset from this straight hinge. Turning past
	// edge-on reverses that curve, revealing the opposite-facing silhouette.
	return [hinge + (x - hinge) * Math.cos(angle), y];
}

export function flagPaths(progress: number, variant: FlagVariant): readonly string[] {
	if (progress <= 0 || progress >= 1) return REST_PATHS;
	const point = (value: Point) => flagPoint(value, progress, variant)
		.map((coordinate) => coordinate.toFixed(3)).join(" ");
	const curves = CONTOURS[1];
	return [REST_PATHS[0], `M${point(curves[0][0])} ${curves.map(
		([, a, b, end]) => `C${point(a)} ${point(b)} ${point(end)}`,
	).join(" ")} Z`];
}
