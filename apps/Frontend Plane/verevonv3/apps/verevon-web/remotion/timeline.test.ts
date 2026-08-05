import { describe, expect, it } from "vitest";
import {
	DURATION_IN_FRAMES,
	FPS,
	getMasterPhase,
	getResetProgress,
	getSignalPosition,
} from "./timeline";

describe("Verevon feature-film master timeline", () => {
	it("uses the expert-approved 12 second master clock", () => {
		expect(FPS).toBe(30);
		expect(DURATION_IN_FRAMES).toBe(360);
	});

	it.each([
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
	] as const)("maps frame %i to %s", (frame, phase) => {
		expect(getMasterPhase(frame)).toBe(phase);
	});

	it("moves one signal monotonically through the four workspaces before reset", () => {
		const checkpoints = [0, 44, 83, 149, 221, 287, 317, 329].map(
			getSignalPosition,
		);

		expect(checkpoints).toEqual([...checkpoints].sort((a, b) => a - b));
		expect(checkpoints.at(-1)).toBe(1);
	});

	it("returns the signal to its opening position for a seamless loop", () => {
		expect(getSignalPosition(359)).toBe(getSignalPosition(0));
		expect(getResetProgress(330)).toBe(0);
		expect(getResetProgress(359)).toBe(1);
	});
});
