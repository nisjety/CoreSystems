import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { REST_PATHS } from "@/components/dev/logo-motion-studies/flag-motion";
import {
	HOMEPAGE_INTRO_DURATION_MS,
	HOMEPAGE_LOCKUP_LANDS_MS,
	HOMEPAGE_LOCKUP_RISE_END_MS,
	HOMEPAGE_LOCKUP_RISE_START_MS,
	HOMEPAGE_LOCKUP_SCALE_END_MS,
	HOMEPAGE_MARK_FADE_END_MS,
	HOMEPAGE_MARK_FADE_START_MS,
	HOMEPAGE_INTRO_SESSION_KEY,
	HOMEPAGE_WORDMARK_END_MS,
	HOMEPAGE_WORDMARK_START_MS,
	HomepageIntro,
} from "./HomepageIntro";

const renderer = vi.hoisted(() => ({ draw: vi.fn(), dispose: vi.fn() }));
const createRenderer = vi.hoisted(() => vi.fn<(
	canvas: HTMLCanvasElement,
	color: string,
	onFailure: () => void,
) => typeof renderer | null>(() => renderer));

vi.mock("@/components/dev/logo-motion-studies/reverse-renderer", () => ({
	createReverseRenderer: createRenderer,
}));

let container: HTMLElement;
let root: Root;
let frames: Map<number, FrameRequestCallback>;
let motionPreference: "full" | "reduce";
let target: HTMLAnchorElement;

beforeEach(() => {
	(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
	window.sessionStorage.clear();
	delete document.documentElement.dataset.homeIntroSession;
	vi.useFakeTimers();
	motionPreference = "full";
	frames = new Map();
	let nextFrame = 0;
	vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
		frames.set(++nextFrame, callback);
		return nextFrame;
	});
	vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => {
		frames.delete(id);
	});
	vi.spyOn(window, "getComputedStyle").mockImplementation(() => ({
		color: "rgb(23, 23, 23)",
		getPropertyValue: (name: string) => name === "--home-intro-motion" ? motionPreference : "",
	}) as CSSStyleDeclaration);
	vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation((() => ({
		clearRect: vi.fn(),
		drawImage: vi.fn(),
	}) as unknown as CanvasRenderingContext2D) as never);
	renderer.draw.mockReset();
	renderer.dispose.mockReset();
	createRenderer.mockReset().mockImplementation(() => renderer);
	target = document.createElement("a");
	target.dataset.siteBrandTarget = "";
	target.href = "/";
	vi.spyOn(target, "getBoundingClientRect").mockReturnValue({
		bottom: 68,
		height: 44,
		left: 24,
		right: 184,
		top: 24,
		width: 160,
		x: 24,
		y: 24,
		toJSON: () => ({}),
	} as DOMRect);
	document.body.append(target);
	container = document.createElement("div");
	document.body.append(container);
	root = createRoot(container);
});

afterEach(() => {
	act(() => root.unmount());
	container.remove();
	target.remove();
	vi.restoreAllMocks();
	vi.unstubAllGlobals();
	vi.useRealTimers();
});

function render(onComplete = vi.fn()) {
	act(() => root.render(<HomepageIntro onComplete={onComplete} />));
	return onComplete;
}

function step(time: number) {
	act(() => {
		const pending = [...frames.values()];
		frames.clear();
		pending.forEach((callback) => callback(time));
	});
}

describe("HomepageIntro", () => {
	it("plays once in a session, then resolves repeat visits without restarting the renderer", () => {
		const firstComplete = render();
		expect(window.sessionStorage.getItem(HOMEPAGE_INTRO_SESSION_KEY)).toBe("1");
		expect(createRenderer).toHaveBeenCalledTimes(1);
		step(0);
		step(HOMEPAGE_INTRO_DURATION_MS);
		expect(firstComplete).toHaveBeenCalledTimes(1);
		expect(container.querySelector<HTMLElement>("[data-home-intro]")?.dataset.time).toBe(String(HOMEPAGE_INTRO_DURATION_MS));

		act(() => root.unmount());
		root = createRoot(container);
		const repeatComplete = render();
		expect(repeatComplete).toHaveBeenCalledTimes(1);
		expect(createRenderer).toHaveBeenCalledTimes(1);
		expect(frames.size).toBe(0);
	});

	it("always plays on the dedicated preview, without changing homepage session state", () => {
		window.sessionStorage.setItem(HOMEPAGE_INTRO_SESSION_KEY, "1");
		const complete = vi.fn();
		act(() => root.render(<HomepageIntro onComplete={complete} playback="preview" />));
		expect(createRenderer).toHaveBeenCalledTimes(1);
		expect(frames.size).toBe(1);
		expect(window.sessionStorage.getItem(HOMEPAGE_INTRO_SESSION_KEY)).toBe("1");
	});

	it("follows the five-second large-lockup handoff clock", () => {
		Object.defineProperty(window, "innerWidth", { configurable: true, value: 1_000 });
		render();
		step(0);
		expect(container.querySelector<HTMLElement>("[data-intro-brand]")?.style.width).toBe("355px");
		expect(container.querySelector<HTMLCanvasElement>("[data-intro-render-source]")?.style.width).toBe("355px");
		expect(container.querySelector<HTMLCanvasElement>("[data-intro-render-source]")?.style.height).toBe("295px");
		expect(container.querySelector<SVGSVGElement>("[data-intro-canonical]")?.parentElement?.style.transform)
			.toBe("translateY(-50%) translateX(0%) scale(1)");
		step(HOMEPAGE_WORDMARK_START_MS - 1);
		expect(container.querySelector<HTMLElement>("[data-intro-letter=\"0\"]")?.style.opacity).toBe("0");

		step(HOMEPAGE_WORDMARK_START_MS + 1);
		expect(container.querySelector<HTMLElement>("[data-intro-letter=\"0\"]")?.style.opacity).toBe("1");

		step(HOMEPAGE_LOCKUP_SCALE_END_MS);
		const intro = container.querySelector<HTMLElement>("[data-home-intro]");
		expect(intro?.style.getPropertyValue("--home-intro-scale")).toBe("1.35");
		step(HOMEPAGE_WORDMARK_END_MS);
		expect(container.querySelector<HTMLElement>("[data-intro-letter=\"6\"]")?.style.transform).toBe("translateY(0em) rotateX(0deg)");
		expect(container.querySelector<SVGSVGElement>("[data-intro-canonical]")?.parentElement?.style.transform)
			.toBe("translateY(-50%) translateX(-36.5%) scale(0.27)");
		expect(intro?.style.getPropertyValue("--home-intro-x")).toBe("0px");
		expect(intro?.style.getPropertyValue("--home-intro-scale")).toBe("1.35");
		expect(HOMEPAGE_LOCKUP_LANDS_MS).toBe(HOMEPAGE_WORDMARK_END_MS);
		step(HOMEPAGE_LOCKUP_RISE_START_MS);
		expect(intro?.style.getPropertyValue("--home-intro-scale")).toBe("1.35");
		step(HOMEPAGE_LOCKUP_RISE_END_MS);
		expect(intro?.style.getPropertyValue("--home-intro-y")).not.toBe("0px");
		expect(intro?.style.getPropertyValue("--home-intro-scale")).toBe("1");
		expect(container.querySelector<SVGSVGElement>("[data-intro-canonical]")?.parentElement?.style.opacity).toBe("1");
		step(HOMEPAGE_MARK_FADE_END_MS);
		expect(container.querySelector<SVGSVGElement>("[data-intro-canonical]")?.parentElement?.style.opacity).toBe("0");
		expect(HOMEPAGE_MARK_FADE_START_MS).toBe(HOMEPAGE_LOCKUP_RISE_END_MS);
		expect(HOMEPAGE_INTRO_DURATION_MS).toBe(5_000);
	});

	it("skips straight to the permanent header when native reduced motion is set", () => {
		motionPreference = "reduce";
		const complete = render();
		expect(complete).toHaveBeenCalledTimes(1);
		expect(createRenderer).not.toHaveBeenCalled();
		expect(frames.size).toBe(0);
		expect(window.sessionStorage.getItem(HOMEPAGE_INTRO_SESSION_KEY)).toBe("1");
	});

	it("stops, releases the renderer, and restores interactivity when motion is reduced live", () => {
		const complete = render();
		step(0);
		motionPreference = "reduce";
		act(() => vi.advanceTimersByTime(100));
		expect(complete).toHaveBeenCalledTimes(1);
		expect(renderer.dispose).toHaveBeenCalledTimes(1);
		expect(frames.size).toBe(0);
		expect(container.querySelector<HTMLElement>("[data-home-intro]")?.dataset.phase).toBe("complete");
	});

	it("falls through to the final static mark when WebGL is unavailable", () => {
		createRenderer.mockReturnValueOnce(null);
		const complete = render();
		expect(complete).toHaveBeenCalledTimes(1);
		const paths = Array.from(container.querySelectorAll("[data-intro-canonical] path"), (path) => path.getAttribute("d"));
		expect(paths).toEqual(REST_PATHS);
		expect(frames.size).toBe(0);
	});

	it("never turns unavailable session storage into a blocking overlay", () => {
		vi.stubGlobal("sessionStorage", {
			getItem: () => { throw new Error("Storage unavailable"); },
			setItem: () => { throw new Error("Storage unavailable"); },
		});
		const complete = render();
		expect(complete).toHaveBeenCalledTimes(1);
		expect(createRenderer).not.toHaveBeenCalled();
		expect(frames.size).toBe(0);
	});

	it("keeps the animated brand as the only painted lockup until the exact terminal handoff", () => {
		const complete = render();
		const frame = container.querySelector<HTMLElement>("[data-intro-frame]");
		Object.defineProperties(frame, {
			offsetHeight: { configurable: true, value: 44 },
			offsetWidth: { configurable: true, value: 160 },
		});
		Object.defineProperties(window, {
			innerHeight: { configurable: true, value: 800 },
			innerWidth: { configurable: true, value: 1000 },
		});
		step(0);
		step(HOMEPAGE_INTRO_DURATION_MS - 1);
		expect(complete).not.toHaveBeenCalled();
		expect(container.querySelector("[data-home-intro]"))?.not.toBeNull();
		step(HOMEPAGE_INTRO_DURATION_MS);
		expect(complete).toHaveBeenCalledTimes(1);
		const intro = container.querySelector<HTMLElement>("[data-home-intro]");
		expect(intro?.dataset.phase).toBe("complete");
		expect(intro?.style.getPropertyValue("--home-intro-x")).toBe("-396px");
		expect(intro?.style.getPropertyValue("--home-intro-y")).toBe("-354px");
		expect(renderer.dispose).toHaveBeenCalledTimes(1);
		expect(frames.size).toBe(0);
	});
});
