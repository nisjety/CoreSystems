export const WORDMARK = "VEREVON";
export const LOCKUP_MOVE_START_MS = 1350;
export const LOCKUP_MOVE_END_MS = 2050;
export const WORD_START_MS = 1740;
export const LETTER_DURATION_MS = 440;
export const LETTER_STAGGER_MS = 45;
export const LOCKUP_END_MS = 2450;

function progress(timeMs: number, start: number, duration: number) {
	const t = Math.max(0, Math.min(1, (timeMs - start) / duration));
	return t ** 3 * (t * (t * 6 - 15) + 10);
}

export function lockupPose(timeMs: number) {
	const p = progress(timeMs, LOCKUP_MOVE_START_MS, LOCKUP_MOVE_END_MS - LOCKUP_MOVE_START_MS);
	return { scale: p === 1 ? 0.27 : 1 - 0.73 * p, xPercent: p === 0 ? 0 : -36.5 * p };
}

export function letterPose(index: number, timeMs: number) {
	const p = progress(timeMs, WORD_START_MS + index * LETTER_STAGGER_MS, LETTER_DURATION_MS);
	return { yEm: 1.12 * (1 - p), angle: p === 1 ? 0 : -90 * (1 - p), opacity: p === 0 ? 0 : 1 };
}

export function paintWordmark(root: HTMLElement, timeMs: number) {
	const pose = lockupPose(timeMs);
	const mark = root.querySelector<HTMLElement>("[data-lockup-mark]");
	if (mark) mark.style.transform = `translateX(${pose.xPercent}%) scale(${pose.scale})`;
	root.querySelectorAll<HTMLElement>("[data-wordmark-letter]").forEach((letter, index) => {
		const pose = letterPose(index, timeMs);
		letter.style.transform = `translateY(${pose.yEm}em) rotateX(${pose.angle}deg)`;
		letter.style.opacity = String(pose.opacity);
	});
	root.dataset.lockupTime = String(Math.round(timeMs));
}
