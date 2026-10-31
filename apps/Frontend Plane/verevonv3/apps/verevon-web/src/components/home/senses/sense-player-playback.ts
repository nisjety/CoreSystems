import { Easing, interpolate } from "remotion";
import type { SenseKind } from "./SenseScene";

type SensePlaybackState = {
	inView: boolean;
	pageVisible: boolean;
	paused: boolean;
	reduced: boolean;
	explicitPlay: boolean;
	completed?: boolean;
};

const playback = {
	delegate: { durationInFrames: 540, loop: true, showControls: true },
	learn: { durationInFrames: 210, loop: false, showControls: false },
	oversee: { durationInFrames: 180, loop: false, showControls: false },
} as const satisfies Record<SenseKind, { durationInFrames: number; loop: boolean; showControls: boolean }>;

export function getSensePlayback(kind: SenseKind) {
	return playback[kind];
}

export function getStageOpacity(kind: SenseKind, frame: number) {
	const easing = Easing.bezier(0.16, 1, 0.3, 1);
	return kind === "delegate"
		? interpolate(frame, [0, 12, 516, 539], [0, 1, 1, 0], { extrapolateLeft: "clamp", extrapolateRight: "clamp", easing })
		: interpolate(frame, [0, 12], [0, 1], { extrapolateLeft: "clamp", extrapolateRight: "clamp", easing });
}

export function shouldPlaySense({ inView, pageVisible, paused, reduced, explicitPlay, completed = false }: SensePlaybackState) {
	return inView && pageVisible && !paused && !completed && (!reduced || explicitPlay);
}

export function formatSenseTime(seconds: number) {
	const whole = Math.floor(Number.isFinite(seconds) ? Math.max(0, seconds) : 0);
	return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}

export function seekFrameFromSeconds(seconds: number, fps: number, durationFrames: number) {
	if (!Number.isFinite(seconds) || fps <= 0 || durationFrames <= 0) return 0;
	return Math.max(0, Math.min(Math.round(seconds * fps), durationFrames - 1));
}
