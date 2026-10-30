export const PRODUCT_LOOP_ENTRANCE_TIMELINE_DURATION = 0.78;
export const PRODUCT_LOOP_ENTRANCE_VIEWPORT_RATIO = 0.78;
export const PRODUCT_LOOP_ENTRANCE_SCROLL_MARGIN =
	`${PRODUCT_LOOP_ENTRANCE_VIEWPORT_RATIO * 100}vh`;

// Keep the interactive stage still long enough for a visitor to enter the demo.
export const PRODUCT_LOOP_STAGE04_HOLD = 0.5;
export const PRODUCT_LOOP_STAGE04_SETTLE_START = 3.26;
export const PRODUCT_LOOP_STAGE04_EXIT = 3.72 + PRODUCT_LOOP_STAGE04_HOLD;
export const PRODUCT_LOOP_STAGE04_ACTIVE_END = 3.96 + PRODUCT_LOOP_STAGE04_HOLD;
export const PRODUCT_LOOP_SETTLE_EVENT = "product-loop:settle-stage";

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

type ProductLoopScrollForTimeInput = Omit<ProductLoopProgressInput, "scrollY"> & {
	time: number;
};

export function getProductLoopScrollYForTime({
	entranceScrollDistance,
	entranceTimelineDuration,
	pinnedScrollRange,
	sectionTop,
	time,
	timelineDuration,
}: ProductLoopScrollForTimeInput) {
	const safeEntranceDistance = Math.max(1, entranceScrollDistance);
	const safePinnedRange = Math.max(1, pinnedScrollRange);
	const entranceStart = sectionTop - safeEntranceDistance;
	const position = Math.min(Math.max(time, 0), Math.max(timelineDuration, 0));

	if (position <= entranceTimelineDuration) {
		const entranceFraction =
			entranceTimelineDuration > 0 ? position / entranceTimelineDuration : 1;
		return entranceStart + entranceFraction * safeEntranceDistance;
	}

	const downstreamDuration = timelineDuration - entranceTimelineDuration;
	const pinnedFraction =
		downstreamDuration > 0
			? (position - entranceTimelineDuration) / downstreamDuration
			: 1;
	return sectionTop + pinnedFraction * safePinnedRange;
}

// Matches the site's locally refined monitor crop on short, wide viewports.
export function getProductLoopContentFrame({
	height,
	width,
}: {
	height: number;
	width: number;
}) {
	const inset = Math.max(Math.min(width * 0.077, 100), (width - 1512) / 2);
	const availableHeight = height - 160;
	const frameWidth = Math.min(width - inset * 2, availableHeight * 2);
	const frameHeight = Math.min(availableHeight, frameWidth * 0.625);
	return {
		height: frameHeight,
		left: (width - frameWidth) / 2,
		top: Math.max(96, (height - frameHeight) / 2),
		width: frameWidth,
	};
}
