import { describe, expect, it } from "vitest";
import {
	formatSenseTime,
	getSensePlayback,
	getStageOpacity,
	seekFrameFromSeconds,
	shouldPlaySense,
} from "./sense-player-playback";

const active = {
	inView: true,
	pageVisible: true,
	paused: false,
	reduced: false,
	explicitPlay: false,
};

describe("sense player scroll playback", () => {
	it("plays as soon as the scroll trigger is active, and pauses when it leaves", () => {
		expect(shouldPlaySense(active)).toBe(true);
		expect(shouldPlaySense({ ...active, inView: false })).toBe(false);
		expect(shouldPlaySense({ ...active, pageVisible: false })).toBe(false);
		expect(shouldPlaySense({ ...active, paused: true })).toBe(false);
	});

	it("honors reduced motion until the visitor explicitly plays", () => {
		expect(shouldPlaySense({ ...active, reduced: true })).toBe(false);
		expect(shouldPlaySense({ ...active, reduced: true, explicitPlay: true })).toBe(true);
	});

	it("converts the player timeline to readable time and safe seek frames", () => {
		expect(formatSenseTime(0)).toBe("0:00");
		expect(formatSenseTime(89 / 30)).toBe("0:02");
		expect(formatSenseTime(18)).toBe("0:18");
		expect(seekFrameFromSeconds(6, 30, 540)).toBe(180);
		expect(seekFrameFromSeconds(50, 30, 540)).toBe(539);
		expect(seekFrameFromSeconds(-2, 30, 540)).toBe(0);
	});
});

describe("sense chapter playback policy", () => {
	it("keeps chapter one looping but lets the other chapters finish on a still", () => {
		expect(getSensePlayback("delegate")).toEqual({ durationInFrames: 540, loop: true, showControls: true });
		expect(getSensePlayback("learn")).toEqual({ durationInFrames: 210, loop: false, showControls: false });
		expect(getSensePlayback("oversee")).toEqual({ durationInFrames: 180, loop: false, showControls: false });
	});

	it("does not resume a finished one-shot chapter on scroll re-entry", () => {
		expect(shouldPlaySense({ ...active, completed: true })).toBe(false);
		expect(shouldPlaySense({ ...active, completed: false })).toBe(true);
	});

	it("keeps the final scene visible while preserving chapter one's dissolve", () => {
		expect(getStageOpacity("delegate", 539)).toBe(0);
		expect(getStageOpacity("learn", 209)).toBe(1);
		expect(getStageOpacity("oversee", 179)).toBe(1);
	});
});
