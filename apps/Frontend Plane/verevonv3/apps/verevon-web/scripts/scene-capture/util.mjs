// Shared helpers for the deterministic scene capture harness.
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const HERE = dirname(fileURLToPath(import.meta.url));
/** apps/verevon-web */
export const WEB_ROOT = resolve(HERE, "..", "..");
/** The SPA checkout that owns the scene runtime (two levels above WEB). */
export const SPA_ROOT = resolve(WEB_ROOT, "..", "..");

/** Raw capture size and the delivered master size (contract section 1 and 4). */
export const RAW_WIDTH = 2560;
export const RAW_HEIGHT = 1600;
export const OUT_WIDTH = 1280;
export const OUT_HEIGHT = 800;
export const FPS = 60;
export const FRAME_MS = 1000 / FPS;
/** Crossfade length at the loop seam, in frames. */
export const DISSOLVE_FRAMES = 30;

/** Exit codes. */
export const EXIT_USAGE = 2;
export const EXIT_FAILURE = 1;

export class CaptureError extends Error {
	constructor(message, { code = EXIT_FAILURE, cause } = {}) {
		super(message, cause ? { cause } : undefined);
		this.name = "CaptureError";
		this.exitCode = code;
	}
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Rejects with a CaptureError naming `label` if `promise` does not settle within `ms`. */
export function withTimeout(promise, ms, label) {
	let timer;
	const timeout = new Promise((_, reject) => {
		timer = setTimeout(
			() => reject(new CaptureError(`Timed out after ${ms} ms: ${label}`)),
			ms,
		);
	});
	return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

export const sha256 = (buf) => createHash("sha256").update(buf).digest("hex");

/** Width and height from a PNG's IHDR chunk. */
export function pngSize(buf) {
	if (buf.length < 24 || buf.readUInt32BE(12) !== 0x49484452) {
		throw new CaptureError("Screenshot is not a PNG");
	}
	return { width: buf.readUInt32BE(16), height: buf.readUInt32BE(20) };
}

/**
 * Device scale factor for a CSS viewport width so that the raw frame is RAW_WIDTH px wide.
 * Chromium keeps the factor as a float32 and rounds the physical size up, so 2560/1120
 * (float32 2.28571439) yields 2560x1601. Take the largest float32 that is <= the exact ratio.
 */
export function deviceScaleFactorFor(cssWidth) {
	const exact = RAW_WIDTH / cssWidth;
	const f32 = new Float32Array([exact]);
	if (f32[0] > exact) {
		const bits = new Uint32Array(f32.buffer);
		bits[0] -= 1;
	}
	return f32[0];
}

export function pad4(n) {
	return String(n).padStart(4, "0");
}

let cachedPlaywright;
/** Playwright lives in the SPA's node_modules (WEB does not depend on it). */
export function loadPlaywright() {
	if (cachedPlaywright) return cachedPlaywright;
	const pkg = resolve(SPA_ROOT, "package.json");
	if (!existsSync(pkg)) {
		throw new CaptureError(`SPA checkout not found at ${SPA_ROOT}`);
	}
	const require = createRequire(pkg);
	try {
		cachedPlaywright = require("@playwright/test");
	} catch (error) {
		throw new CaptureError(
			`Cannot load @playwright/test from ${SPA_ROOT}/node_modules. Run pnpm install in the SPA.`,
			{ cause: error },
		);
	}
	return cachedPlaywright;
}

/** Small structured logger; every line is prefixed so parallel runs stay readable. */
export function makeLogger(prefix, { quiet = false } = {}) {
	const t0 = Date.now();
	const stamp = () => `${((Date.now() - t0) / 1000).toFixed(1).padStart(6)}s`;
	return {
		info: (...a) => {
			if (!quiet) console.log(`[${prefix} ${stamp()}]`, ...a);
		},
		warn: (...a) => console.warn(`[${prefix} ${stamp()}] WARN`, ...a),
		error: (...a) => console.error(`[${prefix} ${stamp()}] ERROR`, ...a),
	};
}
