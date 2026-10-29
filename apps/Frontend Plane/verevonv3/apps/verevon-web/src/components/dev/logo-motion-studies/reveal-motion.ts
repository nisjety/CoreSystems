import { RIBBON_AXIS, RIBBON_NORMAL, RIBBON_ORIGIN, RIBBON_LENGTH, RIBBON_VIEW_SHEAR, RIBBON_THICKNESS, type Vector3 } from "./ribbon-material";

export const REVEAL_DURATION_MS = 2500;
export const REVEAL_HOLD_MS = 260;
export const REVEAL_SETTLED_MS = 2350;
export const TURN_TRAVEL_MS = 320;
export const TURN_DURATION_MS = 1520;
export const TURN_TOP_EXTRA_MS = 250;

export function revealPhase(timeMs: number) {
	return timeMs <= REVEAL_HOLD_MS ? "Verevon" : timeMs < REVEAL_SETTLED_MS ? "Rullende vridning" : "Verevon";
}

export function ribbonTurn(s: number, timeMs: number) {
	const u = Math.max(0, Math.min(1, s / RIBBON_LENGTH));
	const duration = TURN_DURATION_MS + TURN_TOP_EXTRA_MS * u;
	const t = Math.max(0, Math.min(1, (timeMs - REVEAL_HOLD_MS - TURN_TRAVEL_MS * u) / duration));
	const progress = t ** 3 * (t * (t * 6 - 15) + 10);
	const gradient = -2 * Math.PI * 30 * t ** 2 * (1 - t) ** 2
		* (TURN_TRAVEL_MS + TURN_TOP_EXTRA_MS * t) / (RIBBON_LENGTH * duration);
	return { angle: 2 * Math.PI * progress, gradient };
}

// A material fibre rotates through signed depth. It is never width-scaled,
// clamped at edge-on, or tweened into a separately drawn target outline.
export function ribbonPoint([s, q, layer]: Vector3, timeMs: number): Vector3 {
	const { angle, gradient } = ribbonTurn(s, timeMs);
	const cosine = Math.cos(angle);
	const sine = Math.sin(angle);
	const normal: Vector3 = [-sine * RIBBON_NORMAL[0] - q * gradient * RIBBON_AXIS[0],
		-sine * RIBBON_NORMAL[1] - q * gradient * RIBBON_AXIS[1], cosine];
	const length = Math.hypot(...normal);
	return [RIBBON_ORIGIN[0] + s * RIBBON_AXIS[0] + q * cosine * RIBBON_NORMAL[0] + layer * normal[0] / length,
		RIBBON_ORIGIN[1] + s * RIBBON_AXIS[1] + q * cosine * RIBBON_NORMAL[1] + layer * normal[1] / length,
		q * sine + layer * normal[2] / length];
}

const ease = (value: number) => {
	const t = Math.max(0, Math.min(1, value));
	return t ** 3 * (t * (t * 6 - 15) + 10);
};

export function ribbonViewShear(timeMs: number) {
	// Return the camera to the exact frontal plane before the SVG handoff.
	return RIBBON_VIEW_SHEAR * ease((timeMs - REVEAL_HOLD_MS) / 260) * (1 - ease((timeMs - 2000) / 350));
}

export function ribbonView([x, y, centerDepth]: Vector3, timeMs: number): Vector3 {
	// Register the resting FRONT plane, not the middle of its thickness.
	const z = centerDepth - RIBBON_THICKNESS / 2;
	const along = (x - RIBBON_ORIGIN[0]) * RIBBON_AXIS[0] + (y - RIBBON_ORIGIN[1]) * RIBBON_AXIS[1];
	const view = ribbonViewShear(timeMs);
	return [x + z * view * RIBBON_AXIS[0], y + z * view * RIBBON_AXIS[1], z - view * along];
}
