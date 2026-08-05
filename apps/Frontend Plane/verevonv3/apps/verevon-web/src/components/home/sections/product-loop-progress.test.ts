import { describe, expect, it } from "vitest";
import {
	getProductLoopTimelineProgress,
	PRODUCT_LOOP_ENTRANCE_SCROLL_MARGIN,
	PRODUCT_LOOP_ENTRANCE_TIMELINE_DURATION,
} from "./product-loop-progress";

const scene = {
	entranceScrollDistance: 780,
	entranceTimelineDuration: PRODUCT_LOOP_ENTRANCE_TIMELINE_DURATION,
	pinnedScrollRange: 2960,
	sectionTop: 2000,
	timelineDuration: 4.36,
};

describe("product loop scroll progress", () => {
	it("positions the product anchor at the beginning of the entrance reveal", () => {
		expect(PRODUCT_LOOP_ENTRANCE_SCROLL_MARGIN).toBe("78vh");
	});

	it("starts while the section is entering the viewport", () => {
		expect(
			getProductLoopTimelineProgress({
				...scene,
				scrollY: scene.sectionTop - scene.entranceScrollDistance,
			}),
		).toBe(0);

		expect(
			getProductLoopTimelineProgress({
				...scene,
				scrollY: scene.sectionTop - scene.entranceScrollDistance / 2,
			}),
		).toBeCloseTo(0.39 / scene.timelineDuration);
	});

	it("finishes the entrance exactly at the sticky boundary", () => {
		expect(
			getProductLoopTimelineProgress({
				...scene,
				scrollY: scene.sectionTop,
			}),
		).toBeCloseTo(scene.entranceTimelineDuration / scene.timelineDuration);
	});

	it("maps the approved downstream sequence across the pinned range", () => {
		expect(
			getProductLoopTimelineProgress({
				...scene,
				scrollY: scene.sectionTop + scene.pinnedScrollRange / 2,
			}),
		).toBeCloseTo(
			(scene.entranceTimelineDuration +
				(scene.timelineDuration - scene.entranceTimelineDuration) / 2) /
				scene.timelineDuration,
		);

		expect(
			getProductLoopTimelineProgress({
				...scene,
				scrollY: scene.sectionTop + scene.pinnedScrollRange,
			}),
		).toBe(1);
	});

	it("clamps positions before and after the scene", () => {
		expect(
			getProductLoopTimelineProgress({
				...scene,
				scrollY: 0,
			}),
		).toBe(0);
		expect(
			getProductLoopTimelineProgress({
				...scene,
				scrollY: 9000,
			}),
		).toBe(1);
	});
});
