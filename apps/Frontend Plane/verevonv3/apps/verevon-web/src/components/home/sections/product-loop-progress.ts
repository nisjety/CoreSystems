export const PRODUCT_LOOP_ENTRANCE_TIMELINE_DURATION = 0.78;
export const PRODUCT_LOOP_ENTRANCE_VIEWPORT_RATIO = 0.78;
export const PRODUCT_LOOP_ENTRANCE_SCROLL_MARGIN =
	`${PRODUCT_LOOP_ENTRANCE_VIEWPORT_RATIO * 100}vh`;

type ProductLoopProgressInput = {
	entranceScrollDistance: number;
	entranceTimelineDuration: number;
	pinnedScrollRange: number;
	scrollY: number;
	sectionTop: number;
	timelineDuration: number;
};

const clamp01 = (value: number) => Math.min(1, Math.max(0, value));

export function getProductLoopTimelineProgress({
	entranceScrollDistance,
	entranceTimelineDuration,
	pinnedScrollRange,
	scrollY,
	sectionTop,
	timelineDuration,
}: ProductLoopProgressInput) {
	if (timelineDuration <= 0) {
		return 0;
	}

	const safeEntranceDistance = Math.max(1, entranceScrollDistance);
	const safePinnedRange = Math.max(1, pinnedScrollRange);
	const entranceStart = sectionTop - safeEntranceDistance;

	const timelinePosition =
		scrollY <= sectionTop
			? clamp01((scrollY - entranceStart) / safeEntranceDistance) *
				entranceTimelineDuration
			: entranceTimelineDuration +
				clamp01((scrollY - sectionTop) / safePinnedRange) *
					(timelineDuration - entranceTimelineDuration);

	return clamp01(timelinePosition / timelineDuration);
}
