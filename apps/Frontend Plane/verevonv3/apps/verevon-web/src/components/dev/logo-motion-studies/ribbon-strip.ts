import type { Point } from "./flag-motion";

export type Curve = readonly [Point, Point, Point, Point];
export type RibbonSection = { along: number; inner: Point; outer: Point };

function curvePoint(curve: Curve, t: number): Point {
	const weights = [(1 - t) ** 3, 3 * t * (1 - t) ** 2, 3 * t ** 2 * (1 - t), t ** 3];
	const coordinate = (axis: number) => curve.reduce((sum, point, index) => sum + point[axis] * weights[index], 0);
	return [coordinate(0), coordinate(1)];
}

// The logo ribbon is monotone in y. Read its actual source curves rather
// than replacing the approved curl with an approximate hand-drawn shape.
export function contourSection(curves: readonly Curve[], y: number) {
	const crossings = curves.flatMap((curve) => {
		const [start, , , end] = curve;
		if (y < Math.min(start[1], end[1]) || y > Math.max(start[1], end[1])) return [];
		if (start[1] === end[1]) return y === start[1] ? [start[0], end[0]] : [];
		if (y === start[1]) return [start[0]];
		if (y === end[1]) return [end[0]];
		let low = 0;
		let high = 1;
		for (let step = 0; step < 26; step += 1) {
			const middle = (low + high) / 2;
			if ((curvePoint(curve, middle)[1] < y) === (end[1] > start[1])) low = middle;
			else high = middle;
		}
		return [curvePoint(curve, (low + high) / 2)[0]];
	});
	return { left: Math.min(...crossings), right: Math.max(...crossings) };
}

function slope(sections: readonly RibbonSection[], index: number, edge: "inner" | "outer", axis: number) {
	const secant = (from: number, to: number) => (sections[to][edge][axis] - sections[from][edge][axis])
		/ (sections[to].along - sections[from].along);
	if (sections.length === 2) return secant(0, 1);
	if (index === 0) {
		const first = sections[1].along - sections[0].along;
		return secant(0, 1) - first * (secant(1, 2) - secant(0, 1)) / (sections[2].along - sections[0].along);
	}
	if (index === sections.length - 1) {
		const last = sections[index].along - sections[index - 1].along;
		return secant(index - 1, index) + last * (secant(index - 1, index) - secant(index - 2, index - 1))
			/ (sections[index].along - sections[index - 2].along);
	}
	const before = sections[index].along - sections[index - 1].along;
	const after = sections[index + 1].along - sections[index].along;
	// Weight by the opposite interval: inserted source landmarks are not uniform.
	return (after * secant(index - 1, index) + before * secant(index, index + 1)) / (before + after);
}

export function ribbonCurves(sections: readonly RibbonSection[]) {
	return sections.slice(0, -1).map((start, index) => {
		const end = sections[index + 1];
		const third = (end.along - start.along) / 3;
		const edge = (name: "inner" | "outer"): Curve => {
			const control = (section: RibbonSection, at: number, sign: number): Point => [
				section[name][0] + sign * slope(sections, at, name, 0) * third,
				section[name][1] + sign * slope(sections, at, name, 1) * third,
			];
			return [start[name], control(start, index, 1), control(end, index + 1, -1), end[name]];
		};
		return { inner: edge("inner"), outer: edge("outer") };
	});
}
