#!/usr/bin/env node
// Deterministic 60 fps capture of a Verevon product scene (contract section 4).
// See scripts/scene-capture/README.md for the mechanism, outputs and rules for scene authors.
import { parseArgs } from "node:util";
import { CaptureError, EXIT_USAGE } from "./scene-capture/util.mjs";
import { DEFAULTS, captureScene, defaultOutDir, validateViewport } from "./scene-capture/capture.mjs";

const USAGE = `Usage: node scripts/capture-scene.mjs --scene <id> [options]

  --scene <id>           Scene id, e.g. 02-plan or 03-undersok (required)
  --url <url>            Capture from a running server (base URL, or a full page URL).
                         Without it the SPA dev server is started in scene mode and stopped afterwards.
  --port <n>             Port for the self-started dev server (default ${DEFAULTS.port})
  --out <dir>            Output directory (default ${defaultOutDir("<id>")})
  --frames <n>           Frames to render (default: durationMs * 60 / 1000, i.e. 480)
  --poster-frame <n>     Render frame used for the webp poster (default ${DEFAULTS.posterFrame})
  --viewport <WxH>       CSS viewport override (16:10). Default: the scene's __verevonScene.viewport
  --keep-frames          Also write every raw 2560x1600 PNG to <out>/frames/
  --no-encode            Skip ffmpeg (hashes, samples and report only)
  --no-warmup            Skip the warm-up pass
  --no-caret-reset       Do not re-focus the focused text field at t = 0 (its caret blink phase
                         then depends on real time and frames may differ between runs)
  --publish              After all checks pass, copy mp4/webm/poster to public/product-scenes/
                         and append a provenance line to public/product-scenes/SOURCES.md
  --frame-timeout <ms>   Per-frame timeout (default ${DEFAULTS.frameTimeoutMs})
  --ready-timeout <ms>   Timeout for server start and __verevonScene.ready (default ${DEFAULTS.readyTimeoutMs})
  --quiet                Only warnings and errors
  -h, --help             This text

Exit codes: 0 ok, 1 capture failed or output checks failed, 2 usage error.`;

function usageError(message) {
	console.error(`${message}\n\n${USAGE}`);
	process.exit(EXIT_USAGE);
}

function int(name, value, min = 0) {
	if (value === undefined) return undefined;
	if (!/^\d+$/.test(value) || Number(value) < min) usageError(`--${name} must be an integer >= ${min}, got "${value}"`);
	return Number(value);
}

let parsed;
try {
	parsed = parseArgs({
		allowPositionals: false,
		options: {
			scene: { type: "string" },
			url: { type: "string" },
			port: { type: "string" },
			out: { type: "string" },
			frames: { type: "string" },
			"poster-frame": { type: "string" },
			viewport: { type: "string" },
			"keep-frames": { type: "boolean", default: false },
			"no-encode": { type: "boolean", default: false },
			"no-warmup": { type: "boolean", default: false },
			"no-caret-reset": { type: "boolean", default: false },
			publish: { type: "boolean", default: false },
			"frame-timeout": { type: "string" },
			"ready-timeout": { type: "string" },
			quiet: { type: "boolean", default: false },
			help: { type: "boolean", short: "h", default: false },
		},
	}).values;
} catch (error) {
	usageError(error.message);
}
if (parsed.help) {
	console.log(USAGE);
	process.exit(0);
}
if (!parsed.scene) usageError("--scene is required");
if (!/^[a-z0-9][a-z0-9-]*$/.test(parsed.scene)) usageError(`--scene "${parsed.scene}" must be a lowercase id like 02-plan`);
let viewport;
if (parsed.viewport) {
	const m = parsed.viewport.match(/^(\d+)x(\d+)$/);
	if (!m) usageError(`--viewport must look like 1120x700, got "${parsed.viewport}"`);
	viewport = { width: Number(m[1]), height: Number(m[2]) };
	try {
		validateViewport(viewport, "--viewport");
	} catch (error) {
		usageError(error.message);
	}
}
if (parsed.url && !/^https?:\/\//.test(parsed.url)) usageError(`--url must start with http:// or https://`);
if (parsed.publish && parsed["no-encode"]) usageError("--publish needs encoding; drop --no-encode");
if (parsed.frames && parsed["poster-frame"] && Number(parsed["poster-frame"]) >= Number(parsed.frames)) {
	usageError(`--poster-frame ${parsed["poster-frame"]} must be below --frames ${parsed.frames}`);
}

const options = {
	scene: parsed.scene,
	url: parsed.url,
	port: int("port", parsed.port, 1) ?? DEFAULTS.port,
	out: parsed.out,
	frames: int("frames", parsed.frames, 2),
	posterFrame: int("poster-frame", parsed["poster-frame"]),
	viewport,
	keepFrames: parsed["keep-frames"],
	encode: !parsed["no-encode"],
	warmup: !parsed["no-warmup"],
	caretReset: !parsed["no-caret-reset"],
	publish: parsed.publish,
	frameTimeoutMs: int("frame-timeout", parsed["frame-timeout"], 1000) ?? DEFAULTS.frameTimeoutMs,
	readyTimeoutMs: int("ready-timeout", parsed["ready-timeout"], 1000) ?? DEFAULTS.readyTimeoutMs,
	quiet: parsed.quiet,
};

try {
	const report = await captureScene(options);
	if (report.checks) {
		for (const c of report.checks.passed) console.log(`  ok    ${c}`);
		for (const c of report.checks.failed) console.log(`  FAIL  ${c}`);
	}
	const t = report.timings;
	console.log(
		`capture ${report.scene}: ${report.frames} frames, ${t.captureMsPerFrame} ms/frame, total ${(t.totalWallMs / 1000).toFixed(1)} s, status ${report.status}`,
	);
	process.exit(report.status === "ok" ? 0 : 1);
} catch (error) {
	console.error(`capture ${options.scene} FAILED: ${error.message}`);
	if (!(error instanceof CaptureError) && error.stack) console.error(error.stack);
	process.exit(error.exitCode ?? 1);
}
