// Chromium side of the harness: launch, scene loading, virtual clock, BeginFrame stepping and
// the pointer protocol from contract section 2.
//
// Mechanism (chosen empirically, see README.md):
//   * chrome-headless-shell with --deterministic-mode. That meta flag turns on BeginFrame control
//     (--enable-begin-frame-control), --run-all-compositor-stages-before-draw and disables threaded
//     animation/scrolling, checker-imaging and image-animation resync. Nothing is drawn unless the
//     harness sends HeadlessExperimental.beginFrame, and every frame carries the frameTimeTicks the
//     harness chooses, so CSS transitions, CSS/Web animations and requestAnimationFrame advance
//     exactly 1/60 s per frame.
//   * CDP virtual time (Emulation.setVirtualTimePolicy) drives timers, performance.now() and Date.
//     Advances use 'pauseIfNetworkFetchesPending', so a lazy chunk loading over the network is
//     instantaneous in scene time.
//   * The screenshot comes from the same beginFrame call, after all compositor stages ran.
import { CaptureError, FRAME_MS, SPA_ROOT, deviceScaleFactorFor, sleep, withTimeout } from "./util.mjs";
import { commandLineOf, killPids, killTree, waitGone } from "./processes.mjs";

/** Frame i shows every timer due at <= t_i + LEAD_MS (t_i = i * 1000/60 ms). The lead avoids the
 *  exact-boundary race where a timer due exactly at a budget end would only run one frame later. */
export const LEAD_MS = 0.5;

/**
 * Clock model (all three rules were found by measuring run-to-run differences, see README.md).
 *
 * 1. 100 us grid anchored at navigation start. Blink coarsens animation timeline time to 100 us
 *    (TimeClamper, with a per-process random jitter that decides whether a value between two grid
 *    points rounds up or down). The grid is anchored at the document's time origin. A frame time
 *    off the grid was sampled up to 100 us differently per run (0.48 device px in a fast
 *    transition). Every BeginFrame time and every virtual-time target is therefore an exact
 *    multiple of 100 us after navigation start (read in TimeTicks via Performance.getMetrics), and
 *    the scene base (t = 0) is nudged onto that grid with one sub-100 us virtual-time step.
 * 2. The animation clock never lags virtual time (and a no-op rAF loop forces a main frame on every
 *    BeginFrame, otherwise idle frames leave the clock behind). When script forces a style update between frames
 *    (offsetWidth, getBoundingClientRect, getComputedStyle...) and virtual "now" is later than the
 *    last frame time, Blink's AnimationClock jumps to an estimated next frame (last + k * 16.666 ms),
 *    which is off the grid, so animations started there got a jittered start time. Each frame is
 *    therefore rendered with the NEXT frame's time, and timers for frame i run while the clock
 *    already holds a time >= their virtual time: the estimate path is never taken.
 *    Consequence: requestAnimationFrame timestamps and document.timeline lead performance.now() by
 *    one frame (16.7 ms); CSS animation progress in frame i corresponds to scene time t_(i+1).
 * 4. Constant offset from the time origin. Frames that land exactly on a CSS step boundary (an
 *    'steps()'/'step-end' animation whose period is a multiple of 50 ms meets a frame time exactly)
 *    flipped between runs although every observable time was identical: Blink's floating-point
 *    arithmetic is relative to the per-run time origin. Scene t = 0 is therefore always placed at
 *    the same distance after navigation start (next multiple of 5 s, at least 0.5 s after ready).
 * 3. Microsecond-exact conversions. CDP takes milliseconds; Chromium stores int64 microseconds.
 *    Values are sent with a 0.1 us margin so truncating and rounding conversions agree.
 */
export const GRID_US = 100;
const US_MARGIN_MS = 0.0001;
const usToMs = (us) => us / 1000 + US_MARGIN_MS;
const LEAD_US = Math.round(LEAD_MS * 1000);
/** Scene-relative virtual-time target for frame i, on the 100 us grid. */
export const targetUs = (i) => Math.round(((i * 1e6) / 60 + LEAD_US) / GRID_US) * GRID_US;

/** Executable name of every chromium process the harness starts (for PID-reuse-safe checks). */
const CHROMIUM_IMAGE = process.platform === "win32" ? "chrome-headless-shell.exe" : "chrome-headless-shell";

/** Scene t = 0 is placed at a multiple of this many microseconds after navigation start. */
const SCENE_START_STEP_US = 5_000_000;

/** Grace period for Chrome's own shutdown before the harness force-kills it. */
const GRACEFUL_CLOSE_MS = 5_000;

/** Fixed fictional "now": Friday 9 October 2026 12:00 Europe/Oslo (= 10:00 UTC). */
export const FIXED_EPOCH_SECONDS = Date.UTC(2026, 9, 9, 10, 0, 0) / 1000;

export function chromiumArgs({ width, height }) {
	const dsf = deviceScaleFactorFor(width);
	return {
		dsf,
		args: [
			"--deterministic-mode",
			`--window-size=${width},${height}`,
			`--force-device-scale-factor=${dsf.toPrecision(17)}`,
			"--force-color-profile=srgb",
			"--hide-scrollbars",
		],
	};
}

export async function launchBrowser({ chromium, viewport, log }) {
	const { dsf, args } = chromiumArgs(viewport);
	log.info(`launching chrome-headless-shell ${viewport.width}x${viewport.height} css px, dsf ${dsf}`);
	let browser;
	try {
		browser = await chromium.launch({
			channel: "chromium-headless-shell",
			headless: true,
			args,
		});
	} catch (error) {
		throw new CaptureError(
			`Could not launch chrome-headless-shell. Install it with: (cd ${SPA_ROOT}) pnpm exec playwright install chromium-headless-shell\n${error.message}`,
			{ cause: error },
		);
	}
	const browserCdp = await browser.newBrowserCDPSession();
	let commandLine = null;
	const pids = new Set();
	let browserPid = null;
	const recordPids = async () => {
		try {
			const info = await browserCdp.send("SystemInfo.getProcessInfo");
			for (const p of info.processInfo) {
				pids.add(p.id);
				if (p.type === "browser") browserPid = p.id;
			}
		} catch {
			/* best effort */
		}
	};
	await recordPids();
	commandLine = commandLineOf(browserPid); // the exact flags, for the capture report
	return {
		browser,
		dsf,
		args,
		commandLine,
		version: browser.version(),
		recordPids,
		pids,
		/** Last close: { ms, forced }. */
		lastClose: null,
		/**
		 * Closes the browser and proves no chromium process from this run is left. Chrome's graceful
		 * exit took 20-110 s on this machine under load, so after GRACEFUL_CLOSE_MS the recorded
		 * processes are force-killed (the profile is a throwaway temp dir) and polled until gone.
		 */
		async close() {
			const t0 = Date.now();
			await recordPids();
			const closing = browser.close().then(
				() => true,
				(error) => {
					log.warn(`browser.close() failed: ${error.message}`);
					return true;
				},
			);
			const graceful = await Promise.race([closing, sleep(GRACEFUL_CLOSE_MS).then(() => false)]);
			if (!graceful) {
				if (browserPid) killTree(browserPid);
				killPids([...pids]);
			}
			const survivors = await waitGone([...pids], 30_000, CHROMIUM_IMAGE);
			if (survivors.length) killPids(survivors);
			await Promise.race([closing, sleep(5_000)]); // let Playwright remove its temp profile
			this.lastClose = { ms: Date.now() - t0, forced: !graceful };
			return waitGone(survivors, 10_000, CHROMIUM_IMAGE);
		},
	};
}

/**
 * Opens the scene in a fresh context, pumping real-time BeginFrames until
 * window.__verevonScene.ready resolves (requestAnimationFrame only fires on BeginFrames).
 */
export async function openScene({ browser, url, sceneId, readyTimeoutMs, log }) {
	const context = await browser.newContext({
		viewport: null,
		locale: "nb-NO",
		timezoneId: "Europe/Oslo",
		colorScheme: "light",
		reducedMotion: "no-preference",
	});
	const page = await context.newPage();
	const cdp = await context.newCDPSession(page);
	const diagnostics = {
		consoleErrors: [],
		pageErrors: [],
		navigationsAfterReady: [],
		requestsFailed: [],
		// Network requests issued after ready (the timeline). Lazy JS chunks here are a determinism
		// risk: fetching pauses virtual time, but compiling/evaluating the module does not.
		timelineRequests: [],
		// Vite HMR updates swap modules without a navigation: the page is no longer what was loaded.
		hotUpdates: [],
		runningAnimationsAtReady: [],
		frame: null,
	};
	page.on("console", (msg) => {
		const text = msg.text();
		if (msg.type() === "error") diagnostics.consoleErrors.push(text.slice(0, 500));
		if (/^\[vite\] (hot updated|css hot updated|page reload|invalidate)/.test(text)) {
			diagnostics.hotUpdates.push({ frame: diagnostics.frame, text: text.slice(0, 300) });
		}
	});
	page.on("pageerror", (err) => diagnostics.pageErrors.push(String(err?.stack || err).slice(0, 1000)));
	page.on("requestfailed", (req) =>
		diagnostics.requestsFailed.push(`${req.method()} ${req.url()} ${req.failure()?.errorText ?? ""}`),
	);

	let pumping = true;
	let pumpError = null;
	const pump = (async () => {
		while (pumping) {
			try {
				await cdp.send("HeadlessExperimental.beginFrame", { interval: FRAME_MS, noDisplayUpdates: false });
			} catch (error) {
				pumpError = error;
				if (/not supported|wasn't found|BeginFrameControl/i.test(error.message)) {
					pumping = false;
					break;
				}
			}
			await sleep(16);
		}
	})();
	const stopPump = async () => {
		pumping = false;
		await pump;
	};

	try {
		const response = await withTimeout(
			page.goto(url, { waitUntil: "load", timeout: readyTimeoutMs }),
			readyTimeoutMs + 1000,
			`page.goto(${url})`,
		);
		if (response && response.status() >= 400) {
			throw new CaptureError(`${url} answered HTTP ${response.status()}`);
		}
		if (pumpError && !pumping) {
			throw new CaptureError(
				`HeadlessExperimental.beginFrame is not available (${pumpError.message}). The harness needs chrome-headless-shell with --deterministic-mode.`,
			);
		}
		try {
			await page.waitForFunction(() => Boolean(window.__verevonScene), null, {
				timeout: readyTimeoutMs,
				polling: 100,
			});
		} catch (error) {
			throw new CaptureError(
				`window.__verevonScene never appeared on ${url}. Is the dev server running with VITE_SCENE_MODE=1 and does the /scene/:id route exist?`,
				{ cause: error },
			);
		}
		const shape = await page.evaluate(() => {
			const s = window.__verevonScene;
			return {
				id: s.id,
				durationMs: s.durationMs,
				viewport: s.viewport,
				hasReady: Boolean(s.ready && typeof s.ready.then === "function"),
				fns: ["start", "pointerAt", "setCursor"].filter((k) => typeof s[k] !== "function"),
				hasLog: Array.isArray(s.log),
			};
		});
		const problems = [];
		if (!shape.hasReady) problems.push("ready is not a Promise");
		if (shape.fns.length) problems.push(`missing functions: ${shape.fns.join(", ")}`);
		if (!shape.hasLog) problems.push("log is not an array");
		if (!Number.isFinite(shape.durationMs) || shape.durationMs <= 0) problems.push("durationMs is not a positive number");
		if (!shape.viewport || !Number.isFinite(shape.viewport.width) || !Number.isFinite(shape.viewport.height)) {
			problems.push("viewport is missing");
		}
		if (problems.length) {
			throw new CaptureError(`window.__verevonScene does not match contract section 2: ${problems.join("; ")}`);
		}
		if (sceneId && shape.id !== sceneId) {
			throw new CaptureError(`Page reports scene id "${shape.id}" but --scene is "${sceneId}"`);
		}
		await withTimeout(
			page.evaluate(() => window.__verevonScene.ready),
			readyTimeoutMs,
			"window.__verevonScene.ready",
		);
		await stopPump();
		diagnostics.runningAnimationsAtReady = await page.evaluate(() =>
			document
				.getAnimations()
				.filter((a) => a.playState === "running")
				.map((a) => {
					const el = a.effect?.target;
					const name = a.animationName || a.transitionProperty || a.id || a.constructor.name;
					return `${name} on ${el ? el.tagName.toLowerCase() + (el.id ? "#" + el.id : el.classList?.length ? "." + [...el.classList].join(".") : "") : "?"}`;
				}),
		);
		page.on("request", (req) => {
			diagnostics.timelineRequests.push({ frame: diagnostics.frame, type: req.resourceType(), method: req.method(), url: req.url() });
		});
		page.on("framenavigated", (frame) => {
			if (frame === page.mainFrame()) diagnostics.navigationsAfterReady.push(frame.url());
		});
		log.info(`scene "${shape.id}" ready (duration ${shape.durationMs} ms, viewport ${shape.viewport.width}x${shape.viewport.height})`);
		return { context, page, cdp, shape, diagnostics };
	} catch (error) {
		await stopPump();
		await context.close().catch(() => {});
		throw error;
	}
}

/**
 * Caret blink reset, step 1 (in the page, right after virtual time is paused). A focused text
 * field's caret blinks on a Blink timer started, in real time, when the field got focus, so its
 * phase relative to scene t = 0 differs per run (the caret toggled at frame 27 in one run and 28 in
 * the next). Blur and focus inside one task leave that timer running; it is only stopped by the
 * appearance update of a frame. So: blur here, render one no-display frame, then step 2 re-focuses
 * and restores the selection. The timer then restarts at the paused virtual time.
 */
function blurForCaretReset() {
	const el = document.activeElement;
	if (!el || el === document.body) return null;
	const isField = el instanceof HTMLTextAreaElement || (el instanceof HTMLInputElement && typeof el.selectionStart === "number");
	if (!isField && !el.isContentEditable) return null;
	const sel = window.getSelection();
	window.__verevonCaptureCaret = {
		el,
		field: isField ? [el.selectionStart, el.selectionEnd, el.selectionDirection] : null,
		range: !isField && sel && sel.rangeCount ? sel.getRangeAt(0).cloneRange() : null,
	};
	el.blur();
	return el.tagName.toLowerCase() + (el.id ? `#${el.id}` : "");
}

/** Caret blink reset, step 2: re-focus and restore the exact selection. */
function refocusAfterCaretReset() {
	const saved = window.__verevonCaptureCaret;
	delete window.__verevonCaptureCaret;
	if (!saved) return;
	saved.el.focus({ preventScroll: true });
	if (saved.field) saved.el.setSelectionRange(saved.field[0], saved.field[1], saved.field[2]);
	if (saved.range) {
		const sel = window.getSelection();
		sel.removeAllRanges();
		sel.addRange(saved.range);
	}
}

/** Drives scene time: virtual time for tasks, explicit frame times for rendering. */
export class FrameStepper {
	constructor({ page, cdp, frameTimeoutMs, caretReset = true }) {
		this.page = page;
		this.cdp = cdp;
		this.frameTimeoutMs = frameTimeoutMs;
		this.caretReset = caretReset;
		this.caretResetTarget = null;
		this.elapsedUs = 0; // virtual time since scene t = 0
		this.sceneBaseUs = null; // absolute TimeTicks of scene t = 0 (on the origin grid)
		this.lastFrameUs = null; // absolute TimeTicks of the last BeginFrame
		this.clock = null;
		this.mouse = { x: null, y: null, pressed: false };
		this.expiredWaiters = [];
		this.mouseEvents = [];
		cdp.on("Emulation.virtualTimeBudgetExpired", () => {
			const waiters = this.expiredWaiters.splice(0);
			for (const w of waiters) w();
		});
	}

	/** Pauses virtual time at the fixed fictional date, aligns the clocks and calls start() (t = 0). */
	async start() {
		const res = await this.cdp.send("Emulation.setVirtualTimePolicy", {
			policy: "pause",
			initialVirtualTime: FIXED_EPOCH_SECONDS,
		});
		const baseUs = Math.round(res.virtualTimeTicksBase * 1000);
		const originUs = await this.#navigationStartUs();
		const perfNowUs = Math.round((await this.page.evaluate(() => performance.now())) * 1000);
		const skewUs = baseUs - originUs - perfNowUs;
		if (Math.abs(skewUs) > 2 * GRID_US) {
			throw new CaptureError(`time origin check failed: NavigationStart and performance.now() disagree by ${skewUs} us`);
		}
		// Rule 4: scene t = 0 sits at a constant distance after navigation start (the next multiple
		// of SCENE_START_STEP_US, at least 0.5 s away), so every origin-relative time Blink computes is
		// identical across runs, including the floating-point rounding at exact step boundaries.
		const sinceOriginUs = baseUs - originUs;
		const sceneBaseSinceOriginUs = Math.ceil((sinceOriginUs + 500_000) / SCENE_START_STEP_US) * SCENE_START_STEP_US;
		const nudgeUs = sceneBaseSinceOriginUs - sinceOriginUs;
		await this.#advanceBy(nudgeUs, "scene start alignment");
		// CDP honours initialVirtualTime only on the first pause, so Date.now() at t = 0 reads the
		// fixed instant plus this alignment advance (0.5-5.5 s, varies per run; the minute is stable).
		this.sceneBaseUs = originUs + sceneBaseSinceOriginUs;
		this.elapsedUs = 0;
		const dateSkewMs = (await this.page.evaluate(() => Date.now())) - FIXED_EPOCH_SECONDS * 1000;
		this.clock = { baseUs, originUs, nudgeUs, skewUs, sceneBaseSinceOriginUs, dateAtStartMinusFixedMs: dateSkewMs };
		if (this.caretReset) {
			this.caretResetTarget = await this.page.evaluate(blurForCaretReset);
			if (this.caretResetTarget) {
				await this.#beginFrame(this.sceneBaseUs + targetUs(0) - 2 * GRID_US, { noDisplayUpdates: true }, "caret reset frame");
				await this.page.evaluate(refocusAfterCaretReset);
			}
		}
		// Rule 2 needs a main frame on EVERY BeginFrame: Chromium skips the main frame when nothing
		// on the page needs one, Blink's animation clock then stays at an older frame (measured 17-33
		// ms behind on the SPA) and the next forced style update in a task takes the jittered
		// "estimated next frame" path. A no-op requestAnimationFrame loop keeps the clock current.
		await this.page.evaluate(() => {
			if (window.__verevonCaptureTick) return;
			window.__verevonCaptureTick = true;
			const tick = () => requestAnimationFrame(tick);
			requestAnimationFrame(tick);
		});
		// Put the animation clock at frame 0's virtual time before the scene's first timer can run.
		await this.#beginFrame(this.sceneBaseUs + targetUs(0), { noDisplayUpdates: true }, "pre-start frame");
		await this.page.evaluate(() => window.__verevonScene.start());
	}

	async #navigationStartUs() {
		await this.cdp.send("Performance.enable", { timeDomain: "timeTicks" });
		const { metrics } = await this.cdp.send("Performance.getMetrics");
		await this.cdp.send("Performance.disable");
		const nav = metrics.find((m) => m.name === "NavigationStart")?.value;
		if (!Number.isFinite(nav)) throw new CaptureError("Performance.getMetrics returned no NavigationStart");
		return Math.round(nav * 1e6);
	}

	async #advanceBy(budgetUs, label) {
		const expired = new Promise((r) => this.expiredWaiters.push(r));
		await this.cdp.send("Emulation.setVirtualTimePolicy", {
			policy: "pauseIfNetworkFetchesPending",
			budget: usToMs(budgetUs),
		});
		await withTimeout(expired, this.frameTimeoutMs, `virtual time budget for ${label}`);
	}

	async #beginFrame(atUs, params, label) {
		if (this.lastFrameUs !== null && atUs <= this.lastFrameUs) {
			throw new CaptureError(`internal: non-increasing frame time for ${label}`);
		}
		const res = await withTimeout(
			this.cdp.send("HeadlessExperimental.beginFrame", { frameTimeTicks: usToMs(atUs), interval: FRAME_MS, ...params }),
			this.frameTimeoutMs,
			label,
		);
		this.lastFrameUs = atUs;
		return res;
	}

	/** Runs every task due up to frame i's virtual time (t_i + lead, on the grid). */
	async advanceToFrame(i) {
		const target = targetUs(i);
		const budgetUs = target - this.elapsedUs;
		if (budgetUs > 0) await this.#advanceBy(budgetUs, `frame ${i}`);
		this.elapsedUs = Math.max(this.elapsedUs, target);
	}

	/**
	 * Pointer protocol: pointerAt(t) -> real CDP mouse events -> in-page cursor overlay.
	 *
	 * Under BeginFrame control Input.dispatchMouseEvent only acknowledges after a main frame has
	 * processed the event (mousemove is rAF-aligned), so awaiting it before beginFrame deadlocks.
	 * The events are therefore sent without awaiting, then flushInput() issues no-display
	 * "input flush" BeginFrames just before the frame time until Chromium acknowledged them. The
	 * screenshot frame that follows always sees the new :hover / :active state and any click handler.
	 */
	async pointer(tMs, frameIndex) {
		const p = await this.page.evaluate((t) => {
			const s = window.__verevonScene;
			const q = s.pointerAt(t);
			const r = { x: Number(q.x), y: Number(q.y), visible: Boolean(q.visible), pressed: Boolean(q.pressed) };
			s.setCursor(r.x, r.y, r.visible, r.pressed);
			return r;
		}, tMs);
		if (!Number.isFinite(p.x) || !Number.isFinite(p.y)) {
			throw new CaptureError(`pointerAt(${tMs}) returned a non-finite position at frame ${frameIndex}`);
		}
		const pending = [];
		const send = (params) => {
			this.mouseEvents.push({ frame: frameIndex, type: params.type, x: params.x, y: params.y });
			const ack = this.cdp.send("Input.dispatchMouseEvent", params);
			ack.catch(() => {});
			pending.push({ type: params.type, ack });
		};
		const moved = p.x !== this.mouse.x || p.y !== this.mouse.y;
		if (moved || frameIndex === 0) {
			send({
				type: "mouseMoved",
				x: p.x,
				y: p.y,
				button: this.mouse.pressed ? "left" : "none",
				buttons: this.mouse.pressed ? 1 : 0,
			});
		}
		if (p.pressed && !this.mouse.pressed) {
			send({ type: "mousePressed", x: p.x, y: p.y, button: "left", buttons: 1, clickCount: 1 });
		} else if (!p.pressed && this.mouse.pressed) {
			send({ type: "mouseReleased", x: p.x, y: p.y, button: "left", buttons: 0, clickCount: 1 });
		}
		this.mouse = { x: p.x, y: p.y, pressed: p.pressed };
		const flushFrames = pending.length ? await this.flushInput(pending, frameIndex) : 0;
		return { ...p, flushFrames };
	}

	/** Pumps no-display BeginFrames just after the last frame time until every event is acked. */
	async flushInput(pending, frameIndex) {
		const all = Promise.all(pending.map((e) => e.ack));
		let settled = false;
		all.then(() => (settled = true), () => (settled = true));
		const deadline = Date.now() + this.frameTimeoutMs;
		let count = 0;
		while (!settled) {
			if (Date.now() > deadline || count >= 3) {
				throw new CaptureError(`mouse events at frame ${frameIndex} were not acknowledged (${pending.map((e) => e.type).join(", ")})`);
			}
			await this.#beginFrame(this.lastFrameUs + GRID_US, { noDisplayUpdates: true }, `input flush frame for frame ${frameIndex}`);
			count++;
			await Promise.race([all.catch(() => {}), sleep(count === 1 ? 5000 : 2000)]);
		}
		await all; // surfaces a protocol error, if any
		return count;
	}

	/** Renders frame i (rule 2: with the next frame's time). Returns { hasDamage, png | null }. */
	async render(frameIndex, { screenshot }) {
		const params = { noDisplayUpdates: false };
		if (screenshot) params.screenshot = { format: "png" };
		const res = await this.#beginFrame(
			this.sceneBaseUs + targetUs(frameIndex + 1),
			params,
			`beginFrame for frame ${frameIndex}`,
		);
		return {
			hasDamage: res.hasDamage,
			png: res.screenshotData ? Buffer.from(res.screenshotData, "base64") : null,
		};
	}
}
