import { act, StrictMode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { REST_PATHS } from "./flag-motion";
import { RibbonComparison } from "./RibbonComparison";

const rendererA = vi.hoisted(() => ({ draw: vi.fn(), dispose: vi.fn() }));
const rendererB = vi.hoisted(() => ({ draw: vi.fn(), dispose: vi.fn() }));
const createA = vi.hoisted(() => vi.fn(() => rendererA));
const createB = vi.hoisted(() => vi.fn(() => rendererB));
vi.mock("./ribbon-renderer", () => ({ createRibbonRenderer: createA }));
vi.mock("./reverse-renderer", () => ({ createReverseRenderer: createB }));

let container: HTMLElement;
let root: Root;
let frames: Map<number, FrameRequestCallback>;
let preference: "full" | "reduce";

beforeEach(() => {
	createA.mockReset().mockImplementation(() => rendererA);
	createB.mockReset().mockImplementation(() => rendererB);
	for (const renderer of [rendererA, rendererB]) {
		renderer.draw.mockReset();
		renderer.dispose.mockClear();
	}
	(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
	vi.useFakeTimers();
	vi.stubGlobal("IntersectionObserver", undefined);
	preference = "full";
	vi.spyOn(window, "getComputedStyle").mockImplementation(() => ({
		color: "rgb(23, 23, 23)",
		getPropertyValue: (name: string) => name === "--flag-motion" ? preference : "",
	}) as CSSStyleDeclaration);
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

const svg = (variant: "a" | "b") => container.querySelector<SVGSVGElement>(`#logo-root-${variant}`)!;
const paths = (variant: "a" | "b") => Array.from(svg(variant).querySelectorAll("[data-reveal-contour]"), (path) => path.getAttribute("d"));
const expectTime = (time: number) => {
	for (const variant of ["a", "b"] as const) expect(svg(variant).dataset.time).toBe(String(time));
};
const expectCanonical = () => {
	for (const variant of ["a", "b"] as const) {
		expect(paths(variant)).toEqual(REST_PATHS);
		expect(svg(variant).dataset.renderer).toBe("canonical-svg");
	}
};
const render = (props: Partial<Parameters<typeof RibbonComparison>[0]> = {}) => act(() => root.render(<RibbonComparison paused={false} {...props} />));
const step = (time: number) => act(() => {
	const pending = [...frames.values()];
	frames.clear();
	pending.forEach((callback) => callback(time));
});

describe("synchronized ribbon comparison", () => {
	it("prewarms at full measurable size while keeping both canvases hidden during the identical hold", () => {
		const check = (id: string) => {
			const surface = container.querySelector<SVGForeignObjectElement>(`#${id} foreignObject`)!;
			expect(surface.style.display).toBe("inline");
			expect(surface.style.visibility).toBe("hidden");
		};
		rendererA.draw.mockImplementationOnce(() => check("logo-root-a"));
		rendererB.draw.mockImplementationOnce(() => check("logo-root-b"));
		render();
		check("logo-root-a");
		step(0); step(100);
		check("logo-root-a");
		check("logo-root-b");
	});

	it("recreates disposed GPU renderers after StrictMode reconnects effects", () => {
		const instances: Array<{ draw: ReturnType<typeof vi.fn>; dispose: ReturnType<typeof vi.fn> }> = [];
		const createRenderer = () => {
			let disposed = false;
			const renderer = {
				draw: vi.fn(() => {
					if (disposed) throw new Error("Cannot draw with a disposed GPU renderer");
				}),
				dispose: vi.fn(() => { disposed = true; }),
			};
			instances.push(renderer);
			return renderer;
		};
		createA.mockImplementation(createRenderer);
		createB.mockImplementation(createRenderer);
		expect(() => act(() => root.render(<StrictMode><RibbonComparison paused={false} /></StrictMode>))).not.toThrow();
		expect(createA).toHaveBeenCalledTimes(2);
		expect(createB).toHaveBeenCalledTimes(2);
		expect(instances[0].dispose).toHaveBeenCalledTimes(1);
		expect(instances[1].dispose).toHaveBeenCalledTimes(1);
		expect(frames.size).toBe(1);
		step(0);
		step(700);
		expect(instances[2].draw).toHaveBeenLastCalledWith(700);
		expect(instances[3].draw).toHaveBeenLastCalledWith(700);
		expectTime(700);
	});

	it("prewarms both renderers before one shared animation clock starts", () => {
		render();
		for (const variant of ["a", "b"] as const) expect(svg(variant).dataset.variant).toBe(variant);
		expect(createA).toHaveBeenCalledTimes(1);
		expect(createB).toHaveBeenCalledTimes(1);
		expect(rendererA.draw).toHaveBeenLastCalledWith(0);
		expect(rendererB.draw).toHaveBeenLastCalledWith(0);
		const firstClockCall = vi.mocked(window.requestAnimationFrame).mock.invocationCallOrder[0];
		expect(rendererA.draw.mock.invocationCallOrder[0]).toBeLessThan(firstClockCall);
		expect(rendererB.draw.mock.invocationCallOrder[0]).toBeLessThan(firstClockCall);
		expect(frames.size).toBe(1);
		expectTime(0);
	});

	it("draws both active variants at identical times without recreating renderers per frame", () => {
		render();
		step(0);
		for (const time of [500, 700, 1000, 1500, 2000]) {
			step(time);
			expect(rendererA.draw).toHaveBeenLastCalledWith(time);
			expect(rendererB.draw).toHaveBeenLastCalledWith(time);
			expectTime(time);
			expect(frames.size).toBe(1);
		}
		expect(createA).toHaveBeenCalledTimes(1);
		expect(createB).toHaveBeenCalledTimes(1);
	});

	it("uses A's identical hold, motion window and unblended SVG landing for B", () => {
		render();
		step(0);
		step(100);
		expectCanonical();
		step(260);
		expectCanonical();
		step(261);
		for (const variant of ["a", "b"] as const) {
			expect(svg(variant).dataset.renderer).toBe("depth-ribbon");
			expect(svg(variant).querySelector<SVGGElement>("[data-canonical]")!.style.display).toBe("none");
			expect(svg(variant).querySelector<SVGForeignObjectElement>("foreignObject")!.style.opacity).toBe("1");
		}
		step(2349);
		for (const variant of ["a", "b"] as const) {
			expect(svg(variant).dataset.renderer).toBe("depth-ribbon");
			expect(svg(variant).querySelector<SVGGElement>("[data-canonical]")!.style.opacity).toBe("0");
			expect(svg(variant).querySelector<SVGForeignObjectElement>("foreignObject")!.style.opacity).toBe("1");
		}
		step(2350);
		expectTime(2350);
		expectCanonical();
		step(2400);
		expectCanonical();
		step(2500);
		expectTime(2500);
		expect(frames.size).toBe(0);
		expect(vi.getTimerCount()).toBe(0);
	});

	it("preserves the shared position when speed changes, pauses, and resumes", () => {
		render();
		step(0);
		step(600);
		render({ speed: 0.35 });
		step(600);
		step(1200);
		expectTime(810);
		expect(rendererA.draw).toHaveBeenLastCalledWith(810);
		expect(rendererB.draw).toHaveBeenLastCalledWith(810);
		render({ speed: 0.35, paused: true });
		expect(frames.size).toBe(0);
		step(5000);
		expectTime(810);
		render({ speed: 0.35 });
		step(6000);
		expectTime(810);
		step(6060);
		expectTime(831);
		expect(frames.size).toBe(1);
	});

	it("seeks both poses together and resumes from the selected pose", () => {
		render({ paused: true, seekTime: 1200 });
		expectTime(1200);
		expect(rendererA.draw).toHaveBeenLastCalledWith(1200);
		expect(rendererB.draw).toHaveBeenLastCalledWith(1200);
		expect(frames.size).toBe(0);
		render({ paused: true, seekTime: 1600 });
		expectTime(1600);
		expect(rendererA.draw).toHaveBeenLastCalledWith(1600);
		expect(rendererB.draw).toHaveBeenLastCalledWith(1600);
		render();
		step(5000);
		expectTime(1600);
		step(5060);
		expectTime(1660);
	});

	it("supports a deterministic URL pose without starting either clock", () => {
		window.history.replaceState({}, "", "/?t=1100");
		render();
		expectTime(1100);
		expect(rendererA.draw).toHaveBeenLastCalledWith(1100);
		expect(rendererB.draw).toHaveBeenLastCalledWith(1100);
		expect(frames.size).toBe(0);
	});

	it("uses the exact canonical SVGs for static URL capture", () => {
		window.history.replaceState({}, "", "/?static=1");
		render();
		expectTime(2500);
		expectCanonical();
		expect(frames.size).toBe(0);
		expect(vi.getTimerCount()).toBe(0);
	});

	it("allocates no GPU renderer when reduced motion is already active", () => {
		preference = "reduce";
		render();
		expect(createA).not.toHaveBeenCalled();
		expect(createB).not.toHaveBeenCalled();
		expectTime(2500);
		expectCanonical();
		expect(frames.size).toBe(0);
		expect(vi.getTimerCount()).toBe(0);
	});

	it("stops both variants together after a live reduced-motion change", () => {
		render();
		step(0);
		step(700);
		preference = "reduce";
		act(() => vi.advanceTimersByTime(400));
		expectTime(2500);
		expectCanonical();
		expect(frames.size).toBe(0);
		expect(vi.getTimerCount()).toBe(0);
		preference = "full";
		act(() => vi.advanceTimersByTime(800));
		expect(frames.size).toBe(0);
		expectTime(2500);
	});

	it("counts visible delayed frames without stretching either ending", () => {
		render();
		step(0);
		step(200);
		expectTime(200);
		step(2300);
		expect(rendererA.draw).toHaveBeenLastCalledWith(2300);
		expect(rendererB.draw).toHaveBeenLastCalledWith(2300);
		step(2600);
		expectTime(2500);
		expectCanonical();
		expect(frames.size).toBe(0);
		expect(vi.getTimerCount()).toBe(0);
	});

	it("freezes both while hidden and disposes both renderers on unmount", () => {
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
		step(5000);
		expectTime(600);
		step(5060);
		expectTime(660);
		act(() => root.render(null));
		expect(rendererA.dispose).toHaveBeenCalled();
		expect(rendererB.dispose).toHaveBeenCalled();
		expect(frames.size).toBe(0);
		expect(vi.getTimerCount()).toBe(0);
	});
});
