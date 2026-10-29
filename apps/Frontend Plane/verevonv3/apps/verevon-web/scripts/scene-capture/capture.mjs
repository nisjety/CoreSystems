// Orchestrates one deterministic scene capture (contract section 4).
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync, appendFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join, relative, resolve } from "node:path";
import { tmpdir } from "node:os";
import {
	CaptureError,
	FPS,
	FRAME_MS,
	OUT_HEIGHT,
	OUT_WIDTH,
	RAW_HEIGHT,
	RAW_WIDTH,
	SPA_ROOT,
	WEB_ROOT,
	loadPlaywright,
	makeLogger,
	pad4,
	pngSize,
	sha256,
} from "./util.mjs";
import { FIXED_EPOCH_SECONDS, FrameStepper, GRID_US, LEAD_MS, launchBrowser, openScene } from "./browser.mjs";
import { startDevServer, waitForHttp } from "./dev-server.mjs";
import { EncodePipeline, encodePoster, ffmpegVersion, ffprobe, frameSsim } from "./encode.mjs";
import { killPids, killTree } from "./processes.mjs";

const SAMPLE_EVERY = 30;

export const DEFAULTS = {
	port: 5216,
	posterFrame: 420, // render frame 420 = t 7.0 s (contract section 1)
	frameTimeoutMs: 30_000,
	readyTimeoutMs: 120_000,
	warmup: true,
	caretReset: true,
	encode: true,
	publish: false,
};

export function defaultOutDir(sceneId) {
	return join(tmpdir(), "verevon-scene-capture", sceneId);
}

/** Viewport of record for the SPA scenes (written by the runtime author). */
function readSceneConfigViewport() {
	const file = join(SPA_ROOT, "src", "demo", "scene-config.json");
	if (!existsSync(file)) return null;
	try {
		const vp = JSON.parse(readFileSync(file, "utf8")).viewport;
		if (Number.isFinite(vp?.width) && Number.isFinite(vp?.height)) return { width: vp.width, height: vp.height };
	} catch {
		/* fall through */
	}
	return null;
}

export function validateViewport(vp, label) {
	if (!Number.isInteger(vp.width) || !Number.isInteger(vp.height)) {
		throw new CaptureError(`${label}: viewport must be integer CSS px, got ${vp.width}x${vp.height}`);
	}
	if (vp.width * 10 !== vp.height * 16) {
		throw new CaptureError(`${label}: viewport ${vp.width}x${vp.height} is not exactly 16:10`);
	}
	if (vp.width < 640 || vp.width > RAW_WIDTH) {
		throw new CaptureError(`${label}: viewport width ${vp.width} is outside 640..${RAW_WIDTH}`);
	}
}

/** Removes only files this harness writes, so a reused --out never mixes runs. */
function cleanOwnFiles(dir, pattern) {
	if (!existsSync(dir)) return;
	for (const name of readdirSync(dir)) if (pattern.test(name)) rmSync(join(dir, name), { force: true });
}

// Global safety net: whatever happens, kill what this process started.
const emergency = { serverPid: null, browserPids: new Set(), pipelines: new Set() };
function emergencyCleanup() {
	for (const p of emergency.pipelines) p.abort();
	if (emergency.serverPid) killTree(emergency.serverPid);
	if (emergency.browserPids.size) killPids([...emergency.browserPids]);
}
let handlersInstalled = false;
function installHandlers(log) {
	if (handlersInstalled) return;
	handlersInstalled = true;
	process.on("exit", emergencyCleanup);
	for (const sig of ["SIGINT", "SIGTERM", "SIGBREAK", "SIGHUP"]) {
		process.on(sig, () => {
			log.error(`received ${sig}, cleaning up`);
			emergencyCleanup();
			process.exit(130);
		});
	}
}

/**
 * Runs the frame loop. With `capture` it screenshots every frame and feeds `onFrame`;
 * without it, it is the warm-up pass (same protocol, no pixels).
 */
async function runTimeline({ page, cdp, frames, frameTimeoutMs, caretReset, capture, onFrame, log, label, diagnostics }) {
	const stepper = new FrameStepper({ page, cdp, frameTimeoutMs, caretReset });
	await stepper.start();
	const perFrame = [];
	for (let i = 0; i < frames; i++) {
		const tMs = i * FRAME_MS;
		if (diagnostics) diagnostics.frame = i;
		const w0 = performance.now();
		await stepper.advanceToFrame(i);
		const w1 = performance.now();
		const pointer = await stepper.pointer(tMs, i);
		const w2 = performance.now();
		const rendered = await stepper.render(i, { screenshot: capture });
		const w3 = performance.now();
		const extra = capture ? await onFrame(i, tMs, rendered, pointer) : {};
		const w4 = performance.now();
		perFrame.push({
			i,
			tMs: Math.round(tMs * 1000) / 1000,
			pointer,
			hasDamage: rendered.hasDamage,
			wallMs: { advance: +(w1 - w0).toFixed(1), pointer: +(w2 - w1).toFixed(1), render: +(w3 - w2).toFixed(1), write: +(w4 - w3).toFixed(1), total: +(w4 - w0).toFixed(1) },
			...extra,
		});
		if ((i + 1) % 60 === 0 || i === frames - 1) {
			const recent = perFrame.slice(-60).reduce((s, f) => s + f.wallMs.total, 0) / Math.min(60, perFrame.length);
			log.info(`${label}: frame ${i + 1}/${frames} (${recent.toFixed(0)} ms/frame)`);
		}
	}
	return { perFrame, mouseEvents: stepper.mouseEvents, caretReset: stepper.caretResetTarget, clock: stepper.clock };
}

export async function captureScene(options) {
	const opts = { ...DEFAULTS, ...options };
	const log = makeLogger(`capture ${opts.scene}`, { quiet: opts.quiet });
	installHandlers(log);
	const wallStart = performance.now();
	const timings = {};
	const out = resolve(opts.out ?? defaultOutDir(opts.scene));
	const samplesDir = join(out, "samples");
	const framesDir = join(out, "frames");
	mkdirSync(samplesDir, { recursive: true });
	cleanOwnFiles(samplesDir, /^(frame-\d{4}\.png|text-\d{4}\.txt)$/);
	cleanOwnFiles(framesDir, /^frame-\d{4}\.png$/);
	if (opts.keepFrames) mkdirSync(framesDir, { recursive: true });
	const mp4Path = join(out, `${opts.scene}.mp4`);
	const webmPath = join(out, `${opts.scene}.webm`);
	const posterPath = join(out, `${opts.scene}-poster.webp`);
	for (const f of [mp4Path, webmPath, posterPath, join(out, "frame-hashes.json"), join(out, "capture-report.json")]) {
		rmSync(f, { force: true });
	}

	const report = {
		scene: opts.scene,
		startedAt: new Date().toISOString(),
		status: "running",
		host: { platform: process.platform, node: process.version },
		options: { ...opts, out },
		mechanism: null,
		chromium: null,
		viewport: null,
		raw: null,
		output: { width: OUT_WIDTH, height: OUT_HEIGHT, fps: FPS },
		timings,
		cleanup: {},
	};
	const writeReport = () => writeFileSync(join(out, "capture-report.json"), JSON.stringify(report, null, 2));

	let server = null;
	let browserHandle = null;
	let pipeline = null;
	let pendingTeardown = null;
	try {
		if (opts.encode) report.ffmpeg = ffmpegVersion();
		const { chromium } = loadPlaywright();

		// 1. Server.
		let t = performance.now();
		let baseUrl;
		if (opts.url) {
			baseUrl = opts.url.replace(/\/+$/, "");
			const up = await waitForHttp(baseUrl.replace(/\/scene\/.*$/, "") + "/", { timeoutMs: 15_000 });
			if (!up.ok) throw new CaptureError(`--url ${opts.url} is not reachable: ${up.reason}`);
		} else {
			server = await startDevServer({ port: opts.port, log, timeoutMs: opts.readyTimeoutMs });
			emergency.serverPid = server.pid;
			baseUrl = server.url;
			report.devServer = { command: server.command, cwd: SPA_ROOT, env: { VITE_SCENE_MODE: "1" }, url: server.url };
		}
		const url = /\/scene\//.test(baseUrl) || /\.html?(\?|$)/.test(baseUrl) ? baseUrl : `${baseUrl}/scene/${opts.scene}`;
		report.url = url;
		timings.serverMs = Math.round(performance.now() - t);

		// 2. Browser. The viewport fixes window size and device scale factor at launch time.
		let viewport = opts.viewport ?? readSceneConfigViewport() ?? { width: 1280, height: 800 };
		validateViewport(viewport, "viewport");
		t = performance.now();
		browserHandle = await launchBrowser({ chromium, viewport, log });
		for (const pid of browserHandle.pids) emergency.browserPids.add(pid);

		// 3. Warm-up pass (same protocol, no screenshots): lets Vite transform and pre-bundle every
		//    lazily imported module, so a dependency re-optimisation reload cannot hit the real pass.
		const sceneProbe = await openScene({ browser: browserHandle.browser, url, sceneId: opts.scene, readyTimeoutMs: opts.readyTimeoutMs, log });
		const pageViewport = sceneProbe.shape.viewport;
		if (pageViewport.width !== viewport.width || pageViewport.height !== viewport.height) {
			if (opts.viewport) {
				log.warn(`--viewport ${viewport.width}x${viewport.height} overrides the scene's ${pageViewport.width}x${pageViewport.height}`);
			} else {
				validateViewport(pageViewport, "__verevonScene.viewport");
				log.info(`scene viewport is ${pageViewport.width}x${pageViewport.height}; relaunching with it`);
				await sceneProbe.context.close();
				const survivors = await browserHandle.close();
				if (survivors.length) log.warn(`chromium survivors after relaunch close: ${survivors.join(", ")}`);
				viewport = { width: pageViewport.width, height: pageViewport.height };
				browserHandle = await launchBrowser({ chromium, viewport, log });
				for (const pid of browserHandle.pids) emergency.browserPids.add(pid);
				sceneProbe.context = null;
			}
		}
		const frames = opts.frames ?? Math.round((sceneProbe.shape.durationMs * FPS) / 1000);
		if (!Number.isInteger(frames) || frames < 2) throw new CaptureError(`invalid frame count ${frames}`);
		let posterFrame = opts.posterFrame ?? DEFAULTS.posterFrame;
		if (opts.posterFrame == null && posterFrame >= frames) {
			posterFrame = frames - 1;
			log.warn(`only ${frames} frames: poster uses the last render frame ${posterFrame} instead of ${DEFAULTS.posterFrame}`);
		}
		if (opts.encode && (posterFrame < 0 || posterFrame >= frames)) {
			throw new CaptureError(`--poster-frame ${posterFrame} is outside 0..${frames - 1}`);
		}
		report.frames = frames;
		report.posterFrame = posterFrame;
		report.viewport = { css: viewport, deviceScaleFactor: browserHandle.dsf, sceneReported: pageViewport };
		timings.launchMs = Math.round(performance.now() - t);

		t = performance.now();
		if (opts.warmup) {
			const warm = sceneProbe.context
				? sceneProbe
				: await openScene({ browser: browserHandle.browser, url, sceneId: opts.scene, readyTimeoutMs: opts.readyTimeoutMs, log });
			await runTimeline({ page: warm.page, cdp: warm.cdp, frames, frameTimeoutMs: opts.frameTimeoutMs, caretReset: opts.caretReset, capture: false, log, label: "warm-up", diagnostics: warm.diagnostics });
			if (warm.diagnostics.hotUpdates.length) {
				log.warn(`source changed during the warm-up (${warm.diagnostics.hotUpdates[0].text}); the capture pass loads the new version`);
			}
			if (warm.diagnostics.navigationsAfterReady.length) {
				log.warn(`warm-up saw a reload (${warm.diagnostics.navigationsAfterReady.join(", ")}); the capture pass starts fresh anyway`);
			}
			report.warmup = { frames, diagnostics: warm.diagnostics };
			await warm.context.close();
		} else if (sceneProbe.context) {
			await sceneProbe.context.close();
		}
		timings.warmupMs = Math.round(performance.now() - t);

		// 4. Capture pass.
		t = performance.now();
		const scene = await openScene({ browser: browserHandle.browser, url, sceneId: opts.scene, readyTimeoutMs: opts.readyTimeoutMs, log });
		await browserHandle.recordPids();
		for (const pid of browserHandle.pids) emergency.browserPids.add(pid);
		server?.refreshTree();
		timings.openMs = Math.round(performance.now() - t);

		report.mechanism = {
			name: "chrome-headless-shell BeginFrame control + CDP virtual time",
			flags: "--deterministic-mode (BeginFrame control, all compositor stages before draw, no threaded animation/scrolling)",
			timers: "Emulation.setVirtualTimePolicy pause (initialVirtualTime = fixed date), then pauseIfNetworkFetchesPending budgets up to target(i) = round100us(t_i + lead)",
			frameTime: "HeadlessExperimental.beginFrame({ frameTimeTicks: sceneBase + target(i + 1), screenshot: png }); sceneBase sits on the 100 us grid anchored at NavigationStart",
			gridUs: GRID_US,
			frameLeadMs: LEAD_MS,
			frameRule: `frame i (t_i = i*1000/60 ms) shows every timer due at <= t_i + ${LEAD_MS} ms (100 us grid); CSS animations and rAF are sampled one frame later (t_(i+1)) so the animation clock never lags virtual time`,
			inputFlush: "mouse events are sent unawaited, then 1-3 no-display BeginFrames at last frame + k*100 us until Chromium acknowledges them",
			fixedDate: new Date(FIXED_EPOCH_SECONDS * 1000).toISOString(),
			pointer: "pointerAt(t_i) -> Input.dispatchMouseEvent (mouseMoved when the position changes, mousePressed/mouseReleased on edges) -> setCursor(); then beginFrame",
			warmup: opts.warmup,
			caretReset: opts.caretReset ? "focused text field is re-focused after the pause so its caret blink starts at t = 0" : "off",
		};
		report.chromium = { version: browserHandle.version, launchArgs: browserHandle.args, commandLine: browserHandle.commandLine };

		const hashes = [];
		let firstSize = null;
		let lastPng = null;
		let reusedFrames = 0;
		const onFrame = async (i, tMs, rendered) => {
			let png = rendered.png;
			if (!png) {
				if (!lastPng) throw new CaptureError(`frame ${i}: beginFrame returned no screenshot`);
				png = lastPng;
				reusedFrames++;
			}
			lastPng = png;
			if (!firstSize) {
				firstSize = pngSize(png);
				report.raw = { ...firstSize, target: { width: RAW_WIDTH, height: RAW_HEIGHT } };
				if (firstSize.width < RAW_WIDTH - 2 || firstSize.height < RAW_HEIGHT - 2 || firstSize.width > RAW_WIDTH + 2 || firstSize.height > RAW_HEIGHT + 2) {
					throw new CaptureError(`raw frame is ${firstSize.width}x${firstSize.height}, expected ${RAW_WIDTH}x${RAW_HEIGHT}`);
				}
				if (opts.encode) {
					pipeline = new EncodePipeline({ frames, rawSize: firstSize, mp4Path, webmPath, posterFrame, log });
					emergency.pipelines.add(pipeline);
				}
			}
			const hash = sha256(png);
			hashes.push(hash);
			if (opts.keepFrames) writeFileSync(join(framesDir, `frame-${pad4(i)}.png`), png);
			if (i % SAMPLE_EVERY === 0) {
				writeFileSync(join(samplesDir, `frame-${pad4(i)}.png`), png);
				const text = await scene.page.evaluate(() => document.body.innerText);
				writeFileSync(join(samplesDir, `text-${pad4(i)}.txt`), text, "utf8");
			}
			if (pipeline) await pipeline.push(png);
			return { bytes: png.length, sha256: hash };
		};
		const run = await runTimeline({ page: scene.page, cdp: scene.cdp, frames, frameTimeoutMs: opts.frameTimeoutMs, caretReset: opts.caretReset, capture: true, onFrame, log, label: "capture", diagnostics: scene.diagnostics });
		timings.captureMs = Math.round(performance.now() - t);
		timings.captureMsPerFrame = +(timings.captureMs / frames).toFixed(1);

		if (scene.diagnostics.hotUpdates.length) {
			const first = scene.diagnostics.hotUpdates[0];
			throw new CaptureError(
				`the source changed during capture (Vite HMR at frame ${first.frame}: ${first.text}). The frames mix two versions of the scene; rerun when edits have settled.`,
			);
		}
		if (scene.diagnostics.navigationsAfterReady.length) {
			throw new CaptureError(
				`the page navigated during capture (${scene.diagnostics.navigationsAfterReady.join(", ")}), probably a Vite dependency re-optimisation. Rerun.`,
			);
		}
		const sceneLog = await scene.page.evaluate(() => window.__verevonScene.log.slice());
		report.sceneLog = sceneLog;
		report.unmatchedApi = sceneLog.filter((e) => e.kind === "unmatched-api");
		report.diagnostics = { ...scene.diagnostics, reusedFrames };
		delete report.diagnostics.frame;
		const lazyScripts = scene.diagnostics.timelineRequests.filter((r) => r.type === "script" || /\.(m?js|tsx?|jsx)(\?|$)/.test(r.url));
		report.warnings = [];
		if (lazyScripts.length) {
			report.warnings.push(
				`${lazyScripts.length} script(s) loaded during the timeline (first at frame ${lazyScripts[0].frame}: ${lazyScripts[0].url}). Module compile/evaluate is not under virtual time; preload lazy chunks before ready.`,
			);
		}
		if (scene.diagnostics.runningAnimationsAtReady.length) {
			report.warnings.push(
				`animations already running at ready (their phase depends on real time): ${scene.diagnostics.runningAnimationsAtReady.slice(0, 5).join(", ")}`,
			);
		}
		if (report.unmatchedApi.length) report.warnings.push(`${report.unmatchedApi.length} unmatched /api request(s), see unmatchedApi`);
		for (const w of report.warnings) log.warn(w);
		report.mouseEvents = run.mouseEvents;
		report.caretReset = run.caretReset;
		report.clock = run.clock;
		report.perFrame = run.perFrame;
		writeFileSync(join(out, "frame-hashes.json"), JSON.stringify({ scene: opts.scene, frames, algorithm: "sha256 of the raw PNG bytes", hashes }, null, 2));

		// 5. Tear the browser and the server down while the encode tail runs.
		const teardownStart = performance.now();
		const closingHandle = browserHandle;
		browserHandle = null;
		const closingServer = server;
		server = null;
		if (closingServer) report.cleanup.devServerPids = closingServer.refreshTree().map((p) => p.pid);
		const teardown = (async () => {
			await scene.context.close().catch(() => {});
			const [chromiumSurvivors, devServerSurvivors] = await Promise.all([
				closingHandle.close(),
				closingServer ? closingServer.stop() : Promise.resolve(undefined),
			]);
			report.cleanup.chromiumSurvivors = chromiumSurvivors;
			report.cleanup.chromiumPids = [...closingHandle.pids];
			report.cleanup.chromiumClose = closingHandle.lastClose;
			if (closingServer) report.cleanup.devServerSurvivors = devServerSurvivors;
			timings.teardownMs = Math.round(performance.now() - teardownStart);
			emergency.browserPids.clear();
			emergency.serverPid = null;
		})();
		pendingTeardown = teardown;

		// 6. Encode tail, poster, probes.
		if (pipeline) {
			t = performance.now();
			await pipeline.finish();
			emergency.pipelines.delete(pipeline);
			report.ffmpegArgs = pipeline.ffmpegArgs;
			report.loop = {
				dissolveFrames: pipeline.dissolve,
				rule: `output k = render k for k < ${frames - pipeline.dissolve}; output k = round(((D-j)*render k + j*render (k-${frames - pipeline.dissolve})) / D) with j = k-${frames - pipeline.dissolve - 1}, D = ${pipeline.dissolve}`,
			};
			await encodePoster(pipeline.posterRgb, posterPath);
			timings.encodeTailMs = Math.round(performance.now() - t);
			t = performance.now();
			report.ffprobe = { mp4: ffprobe(mp4Path), webm: ffprobe(webmPath), poster: ffprobe(posterPath) };
			report.seam = {
				mp4: frameSsim(mp4Path, frames - 1, 0),
				webm: frameSsim(webmPath, frames - 1, 0),
			};
			timings.probeMs = Math.round(performance.now() - t);
			report.files = Object.fromEntries(
				[mp4Path, webmPath, posterPath].map((f) => [relative(out, f), statSync(f).size]),
			);
		}
		await teardown;
		pendingTeardown = null;
		report.checks = checkOutputs(report, frames);
		timings.totalWallMs = Math.round(performance.now() - wallStart);
		report.status = report.checks && report.checks.failed.length ? "checks-failed" : "ok";
		if (opts.publish) {
			if (report.status !== "ok") throw new CaptureError("not publishing: output checks failed");
			report.published = publish({ scene: opts.scene, mp4Path, webmPath, posterPath, report, log });
		}
		writeReport();
		log.info(`done in ${(timings.totalWallMs / 1000).toFixed(1)} s -> ${out}`);
		return report;
	} catch (error) {
		report.status = "failed";
		report.error = { message: error.message, stack: error.stack };
		throw error;
	} finally {
		if (pendingTeardown) await pendingTeardown.catch(() => {});
		if (pipeline && emergency.pipelines.has(pipeline)) {
			pipeline.abort();
			emergency.pipelines.delete(pipeline);
		}
		if (browserHandle) {
			report.cleanup.chromiumSurvivors = await browserHandle.close().catch(() => []);
			emergency.browserPids.clear();
		}
		if (server) {
			server.refreshTree();
			report.cleanup.devServerSurvivors = await server.stop();
			emergency.serverPid = null;
		}
		report.finishedAt = new Date().toISOString();
		timings.totalWallMs ??= Math.round(performance.now() - wallStart);
		try {
			writeReport();
		} catch {
			/* out dir may be unwritable; the error is already propagating */
		}
	}
}

function checkOutputs(report, frames) {
	const failed = [];
	const passed = [];
	const expect = (ok, what) => (ok ? passed : failed).push(what);
	expect(report.cleanup.chromiumSurvivors?.length === 0, "no chromium process left");
	if (report.cleanup.devServerPids) expect(report.cleanup.devServerSurvivors?.length === 0, "no dev-server process left");
	if (!report.ffprobe) return { passed, failed };
	for (const [kind, codec] of [["mp4", "h264"], ["webm", "vp9"]]) {
		const p = report.ffprobe[kind];
		expect(p.codec_name === codec, `${kind} codec ${p.codec_name} == ${codec}`);
		expect(p.width === OUT_WIDTH && p.height === OUT_HEIGHT, `${kind} size ${p.width}x${p.height}`);
		expect(p.pix_fmt === "yuv420p", `${kind} pix_fmt ${p.pix_fmt}`);
		expect(p.r_frame_rate === "60/1" && p.avg_frame_rate === "60/1", `${kind} frame rate ${p.r_frame_rate} avg ${p.avg_frame_rate}`);
		expect(Number(p.nb_read_frames) === frames, `${kind} frames ${p.nb_read_frames} == ${frames}`);
		expect(report.seam[kind].all >= 0.995, `${kind} seam SSIM(${frames - 1}, 0) ${report.seam[kind].all} >= 0.995`);
	}
	expect(report.ffprobe.mp4.color_space === "bt709", `mp4 color_space ${report.ffprobe.mp4.color_space}`);
	expect(report.ffprobe.poster.codec_name === "webp", `poster codec ${report.ffprobe.poster.codec_name}`);
	return { passed, failed };
}

/** Copies the three media files into WEB/public/product-scenes and records provenance. */
function publish({ scene, mp4Path, webmPath, posterPath, report, log }) {
	const dest = join(WEB_ROOT, "public", "product-scenes");
	mkdirSync(dest, { recursive: true });
	const files = [
		[mp4Path, `${scene}.mp4`],
		[webmPath, `${scene}.webm`],
		[posterPath, `${scene}-poster.webp`],
	];
	for (const [from, name] of files) copyFileSync(from, join(dest, name));
	const head = spawnSync("git", ["rev-parse", "--short", "HEAD"], { cwd: SPA_ROOT, encoding: "utf8" }).stdout?.trim() || "unknown";
	const dirty = spawnSync("git", ["status", "--porcelain", "--", "src/demo"], { cwd: SPA_ROOT, encoding: "utf8" }).stdout?.trim() ? " (src/demo uncommitted)" : "";
	const fixturesDir = join(SPA_ROOT, "src", "demo", "fixtures");
	let fixtureHash = "none";
	if (existsSync(fixturesDir)) {
		const names = readdirSync(fixturesDir, { recursive: true }).map(String).sort();
		fixtureHash = sha256(Buffer.concat(names.filter((n) => statSync(join(fixturesDir, n)).isFile()).map((n) => Buffer.concat([Buffer.from(n), readFileSync(join(fixturesDir, n))])))).slice(0, 16);
	}
	const sources = join(dest, "SOURCES.md");
	if (!existsSync(sources)) {
		writeFileSync(sources, "# Product scene films: provenance\n\nOne line per published capture (written by scripts/capture-scene.mjs --publish).\n\n");
	}
	const line = `- ${scene}: captured ${report.startedAt}, SPA ${head}${dirty}, fixtures sha256 ${fixtureHash}, ${report.frames} frames, chromium ${report.chromium.version}, seam SSIM ${report.seam.mp4.all}\n`;
	appendFileSync(sources, line);
	log.info(`published to ${dest}`);
	return { dir: dest, files: files.map(([, n]) => n), provenance: line.trim() };
}

