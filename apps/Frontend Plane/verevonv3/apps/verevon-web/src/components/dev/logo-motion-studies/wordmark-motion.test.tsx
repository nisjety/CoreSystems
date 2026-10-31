import { describe, expect, it } from "vitest";
import { LOCKUP_END_MS, LOCKUP_MOVE_START_MS, LOCKUP_MOVE_END_MS, WORDMARK, WORD_START_MS, LETTER_DURATION_MS, LETTER_STAGGER_MS, lockupPose, letterPose } from "./wordmark-motion";

describe("Verevon name arrives without changing the approved ribbon", () => {
	it("starts on the full original and reaches one exact lockup before 2.5 seconds", () => {
		expect(WORDMARK).toBe("VEREVON");
		expect(lockupPose(0)).toEqual({ scale: 1, xPercent: 0 });
		expect(lockupPose(LOCKUP_MOVE_START_MS)).toEqual({ scale: 1, xPercent: 0 });
		expect(lockupPose(LOCKUP_MOVE_END_MS)).toEqual({ scale: 0.27, xPercent: -36.5 });
		expect(lockupPose(2500)).toEqual(lockupPose(LOCKUP_MOVE_END_MS));
		expect(LOCKUP_END_MS).toBe(2450);
		expect(WORD_START_MS + LETTER_DURATION_MS + LETTER_STAGGER_MS * 6).toBe(LOCKUP_END_MS);
	});

	it("rolls seven unchanged letters in reading order into an exact common baseline", () => {
		for (let index = 0; index < WORDMARK.length; index++) {
			const start = WORD_START_MS + index * LETTER_STAGGER_MS;
			expect(letterPose(index, start)).toEqual({ yEm: 1.12, angle: -90, opacity: 0 });
			expect(letterPose(index, start + LETTER_DURATION_MS)).toEqual({ yEm: 0, angle: 0, opacity: 1 });
			expect(letterPose(index, 2500)).toEqual({ yEm: 0, angle: 0, opacity: 1 });
		}
		expect(letterPose(0, 2050).yEm).toBeLessThan(letterPose(6, 2050).yEm);
	});

	it("moves monotonically, without overshoot or nonfinite poses", () => {
		let previous = 1;
		for (let time = 0; time <= 2600; time += 10) {
			const pose = lockupPose(time);
			expect(pose.scale).toBeLessThanOrEqual(previous);
			expect(pose.scale).toBeGreaterThanOrEqual(0.27);
			expect(pose.xPercent).toBeGreaterThanOrEqual(-36.5);
			previous = pose.scale;
			for (let index = 0; index < WORDMARK.length; index++) {
				const letter = letterPose(index, time);
				expect(Object.values(letter).every(Number.isFinite)).toBe(true);
				expect(letter.yEm).toBeGreaterThanOrEqual(0);
				expect(letter.angle).toBeGreaterThanOrEqual(-90);
			}
		}
	});

	it("uses eased rather than linear travel and settles with zero endpoint velocity", () => {
		const quarter = LOCKUP_MOVE_START_MS + (LOCKUP_MOVE_END_MS - LOCKUP_MOVE_START_MS) / 4;
		expect(lockupPose(quarter).scale).toBeGreaterThan(1 - 0.73 / 4);
		expect(lockupPose(LOCKUP_MOVE_START_MS + 0.1).scale).toBeCloseTo(1, 10);
		expect(lockupPose(LOCKUP_MOVE_END_MS - 0.1).scale).toBeCloseTo(0.27, 10);
		for (let index = 0; index < WORDMARK.length; index++) {
			const end = WORD_START_MS + index * LETTER_STAGGER_MS + LETTER_DURATION_MS;
			expect(letterPose(index, end - 0.1).angle).toBeCloseTo(0, 7);
		}
	});
});
