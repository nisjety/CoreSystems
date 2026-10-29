import { describe, expect, it } from "vitest";
import { getProductLoopContentFrame } from "./product-loop-progress";
import {
	createSiteMessage,
	getLiveDemoFit,
	LIVE_DEMO_BAR_HEIGHT,
	LIVE_DEMO_COPY,
	LIVE_DEMO_READY_TIMEOUT_MS,
	LIVE_DEMO_SANDBOX,
	type LiveDemoEvent,
	type LiveDemoState,
	parseDemoMessage,
	PRODUCT_LOOP_LIVE_DEMO,
	readDemoMessage,
	reduceLiveDemo,
	shouldAutoLoadLiveDemo,
	shouldPlayLiveDemo,
} from "./product-loop-live-demo";

// The area the live demo is fitted into: the settled film frame, less the 44px bar at its bottom.
const stageArea = (width: number, height: number) => {
	const frame = getProductLoopContentFrame({ height, width });
	return { height: frame.height - LIVE_DEMO_BAR_HEIGHT, width: frame.width };
};

describe("getLiveDemoFit", () => {
	it.each([
		// [viewport width, viewport height, scale, app width, app height] from the site design.
		[1440, 900, 1, 1240, 696],
		[1920, 1080, 1, 1512, 876],
		[1280, 720, 0.806, 1343, 640],
		[1180, 820, 0.906, 1101, 640],
		[1024, 768, 0.777, 1115, 640],
	])("fits a %ix%i viewport at scale %s", (width, height, scale, appWidth, appHeight) => {
		const fit = getLiveDemoFit(stageArea(width, height));
		expect(fit.scale).toBeCloseTo(scale, 2);
		expect(fit.appWidth).toBeCloseTo(appWidth, -1);
		expect(fit.appHeight).toBeCloseTo(appHeight, 0);
	});

	it("never scales above 1, so a large area shows the app at its own size", () => {
		expect(getLiveDemoFit({ height: 1200, width: 2000 })).toEqual({
			appHeight: 1200,
			appWidth: 2000,
			scale: 1,
		});
	});

	it("never makes the app smaller than the minimum app size", () => {
		const fit = getLiveDemoFit({ height: 320, width: 550 });
		expect(fit.scale).toBeCloseTo(0.5);
		expect(fit.appWidth).toBeCloseTo(1100);
		expect(fit.appHeight).toBeCloseTo(640);
	});

	it("returns the minimum app size with scale 1 for an empty area", () => {
		expect(getLiveDemoFit({ height: 0, width: 0 })).toEqual({
			appHeight: 640,
			appWidth: 1100,
			scale: 1,
		});
	});
});

describe("reduceLiveDemo", () => {
	const run = (state: LiveDemoState, events: LiveDemoEvent[]) =>
		events.reduce(reduceLiveDemo, state);

	it("loads near the stage, or waits for a button when the connection is saved", () => {
		expect(reduceLiveDemo("idle", { deferred: false, type: "near" })).toBe("loading");
		expect(reduceLiveDemo("idle", { deferred: true, type: "near" })).toBe("deferred");
		expect(reduceLiveDemo("deferred", { type: "request-load" })).toBe("loading");
	});

	it("becomes ready on the demo's ready message", () => {
		expect(run("loading", [{ type: "ready" }])).toBe("ready");
	});

	it("activates a ready demo and releases it back to ready", () => {
		expect(run("ready", [{ type: "activate" }])).toBe("active");
		expect(run("active", [{ type: "release" }])).toBe("ready");
	});

	it("fails a loading demo on timeout, and a retry loads it again", () => {
		expect(run("loading", [{ type: "timeout" }])).toBe("failed");
		expect(run("failed", [{ type: "request-load" }])).toBe("loading");
	});

	it("fails a demo that reports an error while it loads, and keeps a loaded demo running", () => {
		expect(run("loading", [{ type: "error" }])).toBe("failed");
		expect(run("ready", [{ type: "error" }])).toBe("ready");
		expect(run("active", [{ type: "error" }])).toBe("active");
	});

	it("reloads a loaded demo in place, from ready or active", () => {
		expect(run("ready", [{ type: "reload" }])).toBe("loading");
		expect(run("active", [{ type: "reload" }])).toBe("loading");
	});

	it("keeps the active state when the demo answers a reset with ready", () => {
		expect(run("active", [{ type: "ready" }])).toBe("active");
	});

	it("ignores events that do not apply to the state", () => {
		expect(run("idle", [{ type: "activate" }])).toBe("idle");
		expect(run("ready", [{ type: "release" }])).toBe("ready");
		expect(run("ready", [{ type: "timeout" }])).toBe("ready");
		expect(run("ready", [{ deferred: false, type: "near" }])).toBe("ready");
		expect(run("failed", [{ type: "ready" }])).toBe("failed");
		expect(run("deferred", [{ type: "activate" }])).toBe("deferred");
	});

	it("walks the whole visit: load, ready, activate, release, reload, timeout, retry", () => {
		const path = run("idle", [
			{ deferred: false, type: "near" },
			{ type: "ready" },
			{ type: "activate" },
			{ type: "release" },
			{ type: "reload" },
			{ type: "timeout" },
			{ type: "request-load" },
		]);
		expect(path).toBe("loading");
	});
});

describe("demo messages", () => {
	it("creates the site envelope", () => {
		expect(createSiteMessage("activate")).toEqual({ source: "verevon-demo", type: "activate", v: 1 });
		expect(createSiteMessage("reset")).toEqual({ source: "verevon-demo", type: "reset", v: 1 });
	});

	it("accepts the demo's ready, escape and error messages with the envelope", () => {
		expect(parseDemoMessage({ source: "verevon-demo", type: "ready", v: 1 })).toEqual({
			source: "verevon-demo",
			type: "ready",
			v: 1,
		});
		expect(parseDemoMessage({ source: "verevon-demo", type: "escape", v: 1 })).toEqual({
			source: "verevon-demo",
			type: "escape",
			v: 1,
		});
		expect(parseDemoMessage({ message: "boom", source: "verevon-demo", type: "error", v: 1 })).toEqual({
			message: "boom",
			source: "verevon-demo",
			type: "error",
			v: 1,
		});
	});

	it("rejects data without the envelope or with an unknown type", () => {
		expect(parseDemoMessage(null)).toBeNull();
		expect(parseDemoMessage("ready")).toBeNull();
		expect(parseDemoMessage({ source: "other", type: "ready", v: 1 })).toBeNull();
		expect(parseDemoMessage({ source: "verevon-demo", type: "ready", v: 2 })).toBeNull();
		expect(parseDemoMessage({ source: "verevon-demo", type: "navigate", v: 1 })).toBeNull();
		expect(parseDemoMessage({ type: "ready" })).toBeNull();
	});

	it("types the error text and caps its length", () => {
		expect(parseDemoMessage({ message: 42, source: "verevon-demo", type: "error", v: 1 })).toEqual({
			message: "",
			source: "verevon-demo",
			type: "error",
			v: 1,
		});
		const long = parseDemoMessage({ message: "x".repeat(500), source: "verevon-demo", type: "error", v: 1 });
		expect(long).toMatchObject({ message: "x".repeat(200) });
	});

	it("counts a message only from the frame's own window", () => {
		const frame = { name: "frame" };
		const data = { source: "verevon-demo", type: "ready", v: 1 };
		expect(readDemoMessage({ data, frameWindow: frame, source: frame })).toEqual({
			source: "verevon-demo",
			type: "ready",
			v: 1,
		});
		expect(readDemoMessage({ data, frameWindow: frame, source: { name: "other" } })).toBeNull();
		expect(readDemoMessage({ data, frameWindow: null, source: null })).toBeNull();
	});
});

describe("shouldAutoLoadLiveDemo", () => {
	it("loads unless the visitor saves data or has a slow connection", () => {
		expect(shouldAutoLoadLiveDemo(undefined)).toBe(true);
		expect(shouldAutoLoadLiveDemo(null)).toBe(true);
		expect(shouldAutoLoadLiveDemo({})).toBe(true);
		expect(shouldAutoLoadLiveDemo({ effectiveType: "4g" })).toBe(true);
		expect(shouldAutoLoadLiveDemo({ effectiveType: "3g" })).toBe(true);
		expect(shouldAutoLoadLiveDemo({ saveData: true })).toBe(false);
		expect(shouldAutoLoadLiveDemo({ effectiveType: "2g" })).toBe(false);
		expect(shouldAutoLoadLiveDemo({ effectiveType: "slow-2g" })).toBe(false);
	});
});

describe("shouldPlayLiveDemo", () => {
	it("plays only while loaded, on a visible stage and a visible tab", () => {
		expect(shouldPlayLiveDemo({ stageVisible: true, state: "ready", tabVisible: true })).toBe(true);
		expect(shouldPlayLiveDemo({ stageVisible: true, state: "active", tabVisible: true })).toBe(true);
		expect(shouldPlayLiveDemo({ stageVisible: false, state: "active", tabVisible: true })).toBe(false);
		expect(shouldPlayLiveDemo({ stageVisible: true, state: "ready", tabVisible: false })).toBe(false);
		expect(shouldPlayLiveDemo({ stageVisible: true, state: "loading", tabVisible: true })).toBe(false);
		expect(shouldPlayLiveDemo({ stageVisible: true, state: "failed", tabVisible: true })).toBe(false);
	});
});

describe("live demo configuration and copy", () => {
	it("loads the static bundle from the same origin, without the SPA's own host", () => {
		expect(PRODUCT_LOOP_LIVE_DEMO.src).toBe("/product-demo/index.html");
		expect(PRODUCT_LOOP_LIVE_DEMO.poster).toBe("/product-demo/poster-desktop.webp");
		expect(PRODUCT_LOOP_LIVE_DEMO.posterMobile).toBe("/product-demo/poster-phone.webp");
	});

	it("sandboxes the frame with allow-scripts only", () => {
		expect(LIVE_DEMO_SANDBOX).toBe("allow-scripts");
	});

	it("keeps the timing constants of the plan", () => {
		expect(LIVE_DEMO_BAR_HEIGHT).toBe(44);
		expect(LIVE_DEMO_READY_TIMEOUT_MS).toBe(15000);
	});

	it("names the shield by its visible text, so the label starts with what is seen", () => {
		expect(LIVE_DEMO_COPY.shieldLabel).toContain(LIVE_DEMO_COPY.shieldTitle);
		expect(LIVE_DEMO_COPY.shieldTitle).toBe("Klikk for å prøve");
		expect(LIVE_DEMO_COPY.badge).toBe("Demo · fiktive data");
	});

	it("shows no real company, person or address in the visitor-visible strings", () => {
		const visible = JSON.stringify([PRODUCT_LOOP_LIVE_DEMO, LIVE_DEMO_COPY]);
		expect(visible).not.toMatch(/Aquatiq|Velion|\bIma\b/);
		expect(visible).not.toMatch(/@(?!fjordform\.example)/);
	});
});
