export const FPS = 30;
export const DURATION_IN_FRAMES = 360;
export const WIDTH = 960;
export const HEIGHT = 1200;

export type MasterPhase =
	| "quiet"
	| "task-enters"
	| "agent-builds"
	| "owned-systems-connect"
	| "trusted-systems-connect"
	| "sources-enter"
	| "context-converges"
	| "action-proposed"
	| "approval-hold"
	| "action-completes"
	| "completed-hold"
	| "reset";

const PHASE_STARTS: Array<[number, MasterPhase]> = [
	[0, "quiet"],
	[15, "task-enters"],
	[45, "agent-builds"],
	[84, "owned-systems-connect"],
	[120, "trusted-systems-connect"],
	[150, "sources-enter"],
	[186, "context-converges"],
	[222, "action-proposed"],
	[252, "approval-hold"],
	[288, "action-completes"],
	[318, "completed-hold"],
	[330, "reset"],
];

const clamp01 = (value: number) => Math.min(1, Math.max(0, value));

export const segmentProgress = (
	frame: number,
	startFrame: number,
	endFrame: number,
) => {
	if (endFrame <= startFrame) {
		return frame >= endFrame ? 1 : 0;
	}

	return clamp01((frame - startFrame) / (endFrame - startFrame));
};

export const smoothstep = (progress: number) => {
	const value = clamp01(progress);
	return value * value * (3 - 2 * value);
};

export const easedSegment = (
	frame: number,
	startFrame: number,
	endFrame: number,
) => smoothstep(segmentProgress(frame, startFrame, endFrame));

export const getMasterPhase = (frame: number): MasterPhase => {
	const normalizedFrame = Math.min(
		DURATION_IN_FRAMES - 1,
		Math.max(0, Math.floor(frame)),
	);

	for (let index = PHASE_STARTS.length - 1; index >= 0; index -= 1) {
		const [startFrame, phase] = PHASE_STARTS[index];
		if (normalizedFrame >= startFrame) {
			return phase;
		}
	}

	return "quiet";
};

const travel = (
	frame: number,
	startFrame: number,
	endFrame: number,
	from: number,
	to: number,
) => from + (to - from) * easedSegment(frame, startFrame, endFrame);

export const getResetProgress = (frame: number) =>
	segmentProgress(frame, 330, 359);

export const getLoopOpacity = (frame: number) => {
	const fadeIn = smoothstep(segmentProgress(frame, 0, 12));
	const fadeOut = 1 - smoothstep(segmentProgress(frame, 330, 359));

	return Math.min(fadeIn, fadeOut);
};

export const getSignalPosition = (frame: number) => {
	if (frame < 15) return 0;
	if (frame <= 83) return travel(frame, 15, 83, 0, 0.25);
	if (frame <= 149) return travel(frame, 84, 149, 0.25, 0.5);
	if (frame <= 221) return travel(frame, 150, 221, 0.5, 0.75);
	if (frame <= 287) return travel(frame, 222, 251, 0.75, 0.875);
	if (frame <= 317) return travel(frame, 288, 317, 0.875, 1);
	if (frame <= 329) return 1;

	return travel(frame, 330, 359, 1, 0);
};
