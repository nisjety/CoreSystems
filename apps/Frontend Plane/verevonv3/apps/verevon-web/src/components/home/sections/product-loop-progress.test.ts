import { describe, expect, it } from "vitest";
import {
	getProductLoopContentFrame,
	getProductLoopScrollYForTime,
	getProductLoopTimelineProgress,
	PRODUCT_LOOP_ENTRANCE_SCROLL_MARGIN,
	PRODUCT_LOOP_ENTRANCE_TIMELINE_DURATION,
	PRODUCT_LOOP_STAGE04_ACTIVE_END,
	PRODUCT_LOOP_STAGE04_EXIT,
	PRODUCT_LOOP_STAGE04_HOLD,
	PRODUCT_LOOP_STAGE04_SETTLE_START,
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

// The real timeline after the stage 04 hold: the last tween ends at EXIT + 0.6 + 0.22.
const heldScene = {
	entranceScrollDistance: 780,
	entranceTimelineDuration: PRODUCT_LOOP_ENTRANCE_TIMELINE_DURATION,
	// 436svh section on a 1000px viewport: 4.36 * 1000 - 1000.
	pinnedScrollRange: 3360,
	sectionTop: 2000,
	timelineDuration: PRODUCT_LOOP_STAGE04_EXIT + 0.6 + 0.22,
};

describe("product loop stage 04 hold", () => {
	it("adds half a unit to the still window, from 3.26 to 4.22", () => {
		expect(PRODUCT_LOOP_STAGE04_HOLD).toBe(0.5);
		expect(PRODUCT_LOOP_STAGE04_SETTLE_START).toBe(3.26);
		expect(PRODUCT_LOOP_STAGE04_EXIT).toBeCloseTo(4.22);
		expect(PRODUCT_LOOP_STAGE04_EXIT - PRODUCT_LOOP_STAGE04_SETTLE_START).toBeCloseTo(0.96);
		expect(PRODUCT_LOOP_STAGE04_ACTIVE_END).toBeCloseTo(4.46);
	});

	it("keeps the stage 04 exit before the composer hold ends", () => {
		expect(PRODUCT_LOOP_STAGE04_EXIT).toBeLessThan(heldScene.timelineDuration);
		expect(PRODUCT_LOOP_STAGE04_ACTIVE_END).toBeLessThan(heldScene.timelineDuration);
	});

	it("returns the scroll position that shows a given timeline time", () => {
		for (const time of [0, 0.39, PRODUCT_LOOP_ENTRANCE_TIMELINE_DURATION, 2, 3.74, PRODUCT_LOOP_STAGE04_EXIT, heldScene.timelineDuration]) {
			const scrollY = getProductLoopScrollYForTime({ ...heldScene, time });
			const shown = getProductLoopTimelineProgress({ ...heldScene, scrollY }) * heldScene.timelineDuration;
			expect(shown, `time ${time}`).toBeCloseTo(time, 6);
		}
	});

	it("puts the sticky boundary at the end of the entrance", () => {
		expect(
			getProductLoopScrollYForTime({ ...heldScene, time: PRODUCT_LOOP_ENTRANCE_TIMELINE_DURATION }),
		).toBeCloseTo(heldScene.sectionTop);
	});

	it("clamps times outside the timeline", () => {
		expect(getProductLoopScrollYForTime({ ...heldScene, time: -1 })).toBe(
			heldScene.sectionTop - heldScene.entranceScrollDistance,
		);
		expect(getProductLoopScrollYForTime({ ...heldScene, time: 99 })).toBeCloseTo(
			heldScene.sectionTop + heldScene.pinnedScrollRange,
		);
	});
});

describe("product loop content frame", () => {
	it.each([
		// [viewport width, viewport height, left, top, width, height]
		[1440, 900, 100, 96, 1240, 740],
		[1920, 1080, 204, 96, 1512, 920],
		[1280, 720, 98.56, 96, 1082.88, 560],
		[1180, 820, 90.86, 98.04, 998.28, 623.925],
		[1024, 768, 78.848, 113.28, 866.304, 541.44],
	])("frames a %ix%i viewport", (width, height, left, top, frameWidth, frameHeight) => {
		expect(getProductLoopContentFrame({ height, width })).toEqual({
			height: expect.closeTo(frameHeight, 2),
			left: expect.closeTo(left, 2),
			top: expect.closeTo(top, 2),
			width: expect.closeTo(frameWidth, 2),
		});
	});
});
