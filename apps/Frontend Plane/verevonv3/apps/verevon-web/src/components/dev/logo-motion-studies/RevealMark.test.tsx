import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RevealMark } from "./RevealMark";
import { REST_PATHS } from "./flag-motion";

const renderer = vi.hoisted(() => ({ draw: vi.fn(), dispose: vi.fn() }));
const createRenderer = vi.hoisted(() => vi.fn(() => renderer));
vi.mock("./ribbon-renderer", () => ({ createRibbonRenderer: createRenderer }));

let container: HTMLElement;
let root: Root;
let frames: Map<number, FrameRequestCallback>;

beforeEach(() => {
	createRenderer.mockClear();
	createRenderer.mockImplementation(() => renderer);
	renderer.draw.mockClear();
	renderer.dispose.mockClear();
	(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
	vi.useFakeTimers();
	vi.stubGlobal("IntersectionObserver", undefined);
	frames = new Map();
	let nextFrame = 0;
	vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
		frames.set(++nextFrame, callback);
		return nextFrame;
	});
	vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => { frames.delete(id); });
	container = document.createElement("div");
	document.body.append(container);
	root = createRoot(container);
});

afterEach(() => {
	act(() => root.unmount());
	container.remove();
	window.history.replaceState({}, "", "/");
	vi.restoreAllMocks();
	vi.unstubAllGlobals();
	vi.useRealTimers();
});

const paths = () => Array.from(container.querySelectorAll("[data-reveal-contour]"), (path) => path.getAttribute("d"));
const time = () => container.querySelector("svg")?.getAttribute("data-time");
const step = (time: number) => act(() => {
	const pending = [...frames.values()];
	frames.clear();
	pending.forEach((callback) => callback(time));
});
const render = (props: Partial<Parameters<typeof RevealMark>[0]> = {}) => act(() => root.render(<RevealMark paused={false} {...props} />));

describe("one-shot reveal playback", () => {
	it("warms the GPU before scheduling the roll, keeping the approved SVG visible", () => {
		render();
		expect(createRenderer).toHaveBeenCalledTimes(1);
		expect(renderer.draw).toHaveBeenLastCalledWith(0);
		expect(container.querySelector("svg")?.getAttribute("data-renderer")).toBe("canonical-svg");
		step(0);
		step(300);
		expect(createRenderer).toHaveBeenCalledTimes(1);
		expect(renderer.draw).toHaveBeenLastCalledWith(300);
	});

	it("does not allocate a GPU context when reduced motion is active at mount", () => {
		vi.spyOn(window, "getComputedStyle").mockReturnValue({ getPropertyValue: () => "reduce" } as unknown as CSSStyleDeclaration);
		render();
		expect(createRenderer).not.toHaveBeenCalled();
		expect(paths()).toEqual(REST_PATHS);
		expect(time()).toBe("2500");
		expect(frames.size).toBe(0);
	});

	it("begins on the actual logo and stops scheduling frames after completion", () => {
		render();
		expect(paths()).toEqual(REST_PATHS);
		for (let time = 0; time <= 2520; time += 60) step(time);
		expect(paths()).toEqual(REST_PATHS);
		expect(time()).toBe("2500");
		expect(container.querySelector("svg")?.getAttribute("data-renderer")).toBe("canonical-svg");
		expect(frames.size).toBe(0);
		step(4800);
		render({ speed: 0.35 });
		expect(paths()).toEqual(REST_PATHS);
		expect(frames.size).toBe(0);
	});

	it("uses actual visible time after a delayed frame instead of stretching the ending", () => {
		render();
		step(0);
		step(200);
		expect(container.querySelector("svg")?.getAttribute("data-time")).toBe("200");
		step(2300);
		expect(renderer.draw).toHaveBeenLastCalledWith(2300);
		step(2500);
		expect(paths()).toEqual(REST_PATHS);
		expect(frames.size).toBe(0);
	});

	it("avoids identical path writes during the initial hold and stops polling after completion", () => {
		render();
		const contourPaths = Array.from(container.querySelectorAll("[data-reveal-contour]"));
		const writes = contourPaths.map((path) => vi.spyOn(path, "setAttribute"));
		step(0);
		step(50);
		writes.forEach((write) => expect(write).not.toHaveBeenCalled());
		const styles = vi.spyOn(window, "getComputedStyle");
		for (let time = 100; time <= 2500; time += 60) step(time);
		styles.mockClear();
		act(() => vi.advanceTimersByTime(1600));
		expect(styles).not.toHaveBeenCalled();
		expect(vi.getTimerCount()).toBe(0);
	});

	it("changes playback speed without restarting, pauses, and resumes without jumping", () => {
		render();
		for (let time = 0; time <= 600; time += 60) step(time);
		render({ speed: 0.35 });
		for (let time = 660; time <= 1260; time += 60) step(time);
		expect(renderer.draw).toHaveBeenLastCalledWith(810);
		render({ paused: true });
		expect(frames.size).toBe(0);
		step(5000);
		expect(time()).toBe("810");
		render();
		step(6000);
		expect(time()).toBe("810");
		step(6060);
		expect(renderer.draw).toHaveBeenLastCalledWith(870);
	});

	it("renders a deterministic selected frame with no clock running", () => {
		render({ seekTime: 1250 });
		expect(renderer.draw).toHaveBeenLastCalledWith(1250);
		expect(frames.size).toBe(0);
		render({ seekTime: 0 });
		expect(paths()).toEqual(REST_PATHS);
		expect(container.querySelector("svg")?.getAttribute("data-renderer")).toBe("canonical-svg");
	});

	it("supports deterministic URL capture and canonical static comparison", () => {
		window.history.replaceState({}, "", "/?t=1100");
		render();
		expect(renderer.draw).toHaveBeenLastCalledWith(1100);
		expect(frames.size).toBe(0);
		window.history.replaceState({}, "", "/?static=1");
		render({ speed: 0.35 });
		expect(paths()).toEqual(REST_PATHS);
		expect(container.querySelector("svg")?.getAttribute("data-renderer")).toBe("canonical-svg");
	});

	it("jumps to the real logo for reduced motion, including a live preference change", () => {
		let preference = "full";
		vi.spyOn(window, "getComputedStyle").mockReturnValue({ getPropertyValue: () => preference } as unknown as CSSStyleDeclaration);
		render();
		step(0);
		step(60);
		preference = "reduce";
		act(() => vi.advanceTimersByTime(400));
		expect(paths()).toEqual(REST_PATHS);
		expect(frames.size).toBe(0);
		preference = "full";
		act(() => vi.advanceTimersByTime(400));
		expect(frames.size).toBe(0);
	});

	it("freezes while hidden and cleans up its clock and listeners", () => {
		let hidden = false;
		vi.spyOn(document, "hidden", "get").mockImplementation(() => hidden);
		render();
		step(0);
		step(600);
		hidden = true;
		act(() => document.dispatchEvent(new Event("visibilitychange")));
		expect(frames.size).toBe(0);
		hidden = false;
		act(() => document.dispatchEvent(new Event("visibilitychange")));
		expect(frames.size).toBe(1);
		act(() => root.render(null));
		expect(renderer.dispose).toHaveBeenCalled();
		expect(frames.size).toBe(0);
		expect(vi.getTimerCount()).toBe(0);
	});

	it("falls back to the exact SVG if the depth renderer is unavailable", () => {
		createRenderer.mockReturnValueOnce(null as unknown as typeof renderer);
		render({ seekTime: 1200 });
		expect(paths()).toEqual(REST_PATHS);
		expect(time()).toBe("2500");
		expect(container.querySelector("[data-canonical]")?.getAttribute("style")).toBe("display: inline;");
		expect(frames.size).toBe(0);
	});
});
