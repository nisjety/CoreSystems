import { ribbonPoint, ribbonTurn } from "./reveal-motion";
import type { Vector3 } from "./ribbon-material";

export function reverseTurn(s: number, timeMs: number) {
	const { angle, gradient } = ribbonTurn(s, timeMs);
	return { angle: -angle, gradient: -gradient };
}

export function reversePoint([s, q, layer]: Vector3, timeMs: number): Vector3 {
	// Reflect the original turn through its material plane, including its
	// thickness normal. Timing, fibres and approved geometry stay identical.
	const [x, y, z] = ribbonPoint([s, q, -layer], timeMs);
	return [x, y, -z];
}
