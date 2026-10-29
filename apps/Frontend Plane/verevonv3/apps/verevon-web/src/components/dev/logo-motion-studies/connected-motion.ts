import { RIBBON_AXIS, RIBBON_NORMAL, RIBBON_ORIGIN, RIBBON_LENGTH, RIBBON_THICKNESS, RIBBON_VIEW_SHEAR, type Vector3 } from "./ribbon-material";
import { CONNECTED_FOOT, CONNECTED_HINGE, connectedEase, connectedEaseSlope, type ConnectedVertex } from "./connected-material";

export const CONNECTED_SETTLED_MS = 2400;
export const CONNECTED_HOLD_MS = 260;
export const CONNECTED_TRAVEL_MS = 200;
export const CONNECTED_TURN_MS = 1770;
export const CONNECTED_TOP_EXTRA_MS = 120;
export const CONNECTED_RELEASE_MS = 2070;

export function attachmentAmount(timeMs: number) {
	return connectedEase(timeMs / 200) * (1 - connectedEase((timeMs - CONNECTED_RELEASE_MS) / (CONNECTED_SETTLED_MS - CONNECTED_RELEASE_MS)));
}

export function connectedHandoff(timeMs: number) {
	// The twist is already finished. Smooth only the differing SVG/WebGL
	// edge antialiasing during the final20ms, never the ribbon attachment.
	return connectedEase((timeMs - 2380) / (CONNECTED_SETTLED_MS - 2380));
}

export function connectedTurn(s: number, timeMs: number) {
	const u = Math.max(0, Math.min(1, s / RIBBON_LENGTH));
	const delay = connectedEase(u);
	const duration = CONNECTED_TURN_MS + CONNECTED_TOP_EXTRA_MS * delay;
	const t = Math.max(0, Math.min(1, (timeMs - CONNECTED_HOLD_MS - CONNECTED_TRAVEL_MS * delay) / duration));
	const base = -2 * Math.PI * connectedEase(t);
	const baseSlope = 2 * Math.PI * connectedEaseSlope(t) * (CONNECTED_TRAVEL_MS + CONNECTED_TOP_EXTRA_MS * t)
		* connectedEaseSlope(u) / (RIBBON_LENGTH * duration);
	const pin = connectedEase((s - CONNECTED_FOOT) / (CONNECTED_HINGE - CONNECTED_FOOT));
	const pinSlope = connectedEaseSlope((s - CONNECTED_FOOT) / (CONNECTED_HINGE - CONNECTED_FOOT)) / (CONNECTED_HINGE - CONNECTED_FOOT);
	return { angle: base * pin, gradient: baseSlope * pin + base * pinSlope };
}

export function connectedPoint([restS, restQ, layer, s, attachedQ]: ConnectedVertex, timeMs: number): Vector3 {
	const attachment = attachmentAmount(timeMs);
	const q = s < 0 ? attachedQ : restQ + (attachedQ - restQ) * attachment;
	const { angle, gradient } = connectedTurn(s, timeMs);
	const sine = Math.sin(angle);
	const cosine = Math.cos(angle);
	const normal: Vector3 = [-sine * RIBBON_NORMAL[0] - q * gradient * RIBBON_AXIS[0],
		-sine * RIBBON_NORMAL[1] - q * gradient * RIBBON_AXIS[1], cosine];
	const length = Math.hypot(...normal);
	const point: Vector3 = [RIBBON_ORIGIN[0] + s * RIBBON_AXIS[0] + q * cosine * RIBBON_NORMAL[0] + layer * normal[0] / length,
		RIBBON_ORIGIN[1] + s * RIBBON_AXIS[1] + q * cosine * RIBBON_NORMAL[1] + layer * normal[1] / length,
		q * sine + layer * normal[2] / length - RIBBON_THICKNESS / 2];
	if (s >= restS) return point;
	// The authorized gap return retracts ALL collar coordinates. Leaving its
	// rotated thickness behind would produce a tiny detached bead at the tip.
	return [RIBBON_ORIGIN[0] + (point[0] - RIBBON_ORIGIN[0]) * attachment,
		RIBBON_ORIGIN[1] + (point[1] - RIBBON_ORIGIN[1]) * attachment, point[2] * attachment];
}

export function connectedViewShear(timeMs: number) {
	return RIBBON_VIEW_SHEAR * connectedEase((timeMs - CONNECTED_HOLD_MS) / 300)
		* (1 - connectedEase((timeMs - 1980) / (CONNECTED_SETTLED_MS - 1980)));
}

export function connectedView([x, y, z]: Vector3, timeMs: number): Vector3 {
	const view = connectedViewShear(timeMs);
	const s = (x - RIBBON_ORIGIN[0]) * RIBBON_AXIS[0] + (y - RIBBON_ORIGIN[1]) * RIBBON_AXIS[1];
	return [x + z * view * RIBBON_AXIS[0], y + z * view * RIBBON_AXIS[1], z - view * s];
}
