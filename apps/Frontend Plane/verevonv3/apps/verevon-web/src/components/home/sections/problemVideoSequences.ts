export type ProblemVideoShot = {
	duration: number;
	src: string;
};

// The order is intentionally narrative: a person waits, time passes, an
// external system holds the movement, the journey stretches, and the result
// is still somewhere on the way.
export const waitingProblemShots: ProblemVideoShot[] = [
	{
		duration: 6,
		src: "/verevon-vibe/problem-waiting/waiting-room-ai.mp4",
	},
	{
		duration: 5,
		src: "/verevon-vibe/problem-waiting/clock-ai.mp4",
	},
	{
		duration: 7,
		src: "/verevon-vibe/problem-waiting/traffic-light-higgsfield.mp4",
	},
	{
		duration: 8,
		src: "/verevon-vibe/problem-waiting/corridor-web.mp4",
	},
	{
		duration: 9,
		src: "/verevon-vibe/problem-waiting/baggage-carousel-higgsfield.mp4",
	},
];

export const scatteredProblemShots: ProblemVideoShot[] = [
	{
		duration: 9,
		src: "/verevon-vibe/problem-scattered/archive-catalog-6549263.mp4",
	},
	{
		duration: 7,
		src: "/verevon-vibe/problem-scattered/screens-real.mp4",
	},
	{
		duration: 6,
		src: "/verevon-vibe/problem-scattered/hands-ai-final.mp4",
	},
	{
		duration: 9,
		src: "/verevon-vibe/problem-scattered/overhead-table-real.mp4",
	},
	{
		duration: 7,
		src: "/verevon-vibe/problem-scattered/document-wall-real.mp4",
	},
];

/**
 * The sequence starts with automated action and resolves with a visible,
 * deliberate human approval.
 */
export const controlProblemShots: ProblemVideoShot[] = [
	{
		duration: 7,
		src: "/verevon-vibe/problem-control/autonomous-car-ai.mp4",
	},
	{
		duration: 7,
		src: "/verevon-vibe/problem-control/robotic-arm-action.mp4",
	},
	{
		duration: 6,
		src: "/verevon-vibe/problem-control/retro-control-room-ai.mp4",
	},
	{
		duration: 8,
		src: "/verevon-vibe/problem-control/human-approval-breaker.mp4",
	},
];
