import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { LogoMotionStudies } from "./LogoMotionStudies";
import { FlagMark } from "./FlagMark";
import { flagPaths, flagPoint, FLAG_STUDIES, REST_PATHS } from "./flag-motion";

vi.mock("./ribbon-renderer", () => ({ createRibbonRenderer: () => ({ draw: vi.fn(), dispose: vi.fn() }) }));
vi.mock("./reverse-renderer", () => ({ createReverseRenderer: () => ({ draw: vi.fn(), dispose: vi.fn() }) }));
vi.mock("./ribbon-copy", () => ({ createRibbonCopy: () => ({ draw: vi.fn(), dispose: vi.fn() }) }));

let container: HTMLElement;
let root: Root | undefined;

beforeAll(() => {
	(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
});

afterEach(() => {
	if (root) act(() => root?.unmount());
	root = undefined;
	container?.remove();
	vi.restoreAllMocks();
	vi.unstubAllGlobals();
	vi.useRealTimers();
});

function mount(component = <LogoMotionStudies />) {
	container = document.createElement("div");
	document.body.append(container);
	root = createRoot(container);
	act(() => root?.render(component));
}

describe("continuous flag geometry", () => {
	for (const { id } of FLAG_STUDIES) {
		it(`${id} keeps the exact silhouette at rest, the left leg rigid and the join anchored`, () => {
			expect(flagPaths(0, id)).toEqual(REST_PATHS);
			expect(flagPaths(1, id)).toEqual(REST_PATHS);
			expect(flagPaths(0.4, id)).not.toEqual(REST_PATHS);
			for (const t of [0.2, 0.4, 0.6, 0.8]) {
				expect(flagPaths(t, id)[0]).toBe(REST_PATHS[0]);
				expect(flagPoint([74.4, 61.7], t, id)).toEqual([74.4, 61.7]);
				expect(flagPaths(t, id).join()).not.toMatch(/NaN|Infinity/);
			}
		});

		it(`${id} turns the curve edge-on and reverses its face without translating the hinge`, () => {
			const center = [80.325, 48.85] as const;
			const section = (time: number) => {
				const a = flagPoint([center[0] - 2, center[1]], time, id);
				const b = flagPoint([center[0] + 2, center[1]], time, id);
				return { width: b[0] - a[0], a, b };
			};
			const samples = Array.from({ length: 201 }, (_, i) => section(i / 200));
			expect(section(0).width).toBeCloseTo(4);
			expect(Math.min(...samples.map(({ width }) => Math.abs(width)))).toBeLessThan(0.08);
			expect(Math.min(...samples.map(({ width }) => width))).toBeLessThan(-3.8);
			for (const { a, b } of samples) {
				expect((a[0] + b[0]) / 2).toBeCloseTo(center[0], 8);
				expect((a[1] + b[1]) / 2).toBeCloseTo(center[1], 8);
			}
		});

		it(`${id} carries the pinch along the curve rather than flattening everything at once`, () => {
			const firstTurn = (along: number) => {
				const center = [74.4 + 11.85 * along, 61.7 - 25.7 * along];
				return Array.from({ length: 501 }, (_, i) => i / 500).find((time) => {
					const a = flagPoint([center[0] - 1, center[1]], time, id);
					const b = flagPoint([center[0] + 1, center[1]], time, id);
					return b[0] - a[0] < 0;
				});
			};
			const start = firstTurn(0.15);
			const end = firstTurn(0.85);
			expect(start).toBeDefined();
			expect(end).toBeDefined();
			expect(end! - start!).toBeGreaterThan(0.025);
		});

		it(`${id} rotates each material section uniformly without extra cross-width folds`, () => {
			for (const time of [0.25, 0.35, 0.5, 0.65, 0.75]) {
				for (const y of [36, 42, 50, 56, 60]) {
					const left = flagPoint([66, y], time, id);
					const right = flagPoint([94, y], time, id);
					const middle = flagPoint([80, y], time, id);
					expect(middle[0]).toBeCloseTo((left[0] + right[0]) / 2, 10);
					expect(middle[1]).toBe(y);
				}
			}
		});
	}
});

describe("flag study playback", () => {
	function playback() {
		vi.useFakeTimers();
		vi.stubGlobal("IntersectionObserver", undefined);
		let nextFrame = 0;
		const frames = new Map<number, FrameRequestCallback>();
		vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
			frames.set(++nextFrame, callback);
			return nextFrame;
		});
		vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => { frames.delete(id); });
		const step = (time: number) => act(() => {
			const pending = [...frames.values()];
			frames.clear();
			pending.forEach((callback) => callback(time));
		});
		const paths = () => Array.from(container.querySelectorAll("[data-flag-contour]"), (path) => path.getAttribute("d"));
		return { frames, step, paths };
	}

	it("slows the same geometry without changing the rollover", () => {
		const { step, paths } = playback();
		mount(<FlagMark variant="flag" paused={false} speed={0.35} />);
		for (let time = 0; time <= 1200; time += 60) step(time);
		expect(paths()).toEqual(flagPaths(420 / 2400, "flag"));
	});

	it("freezes geometry, resumes without a time jump, and cleans up on unmount", () => {
		const { frames, step, paths } = playback();
		mount(<FlagMark variant="flag" paused={false} />);
		for (let time = 0; time <= 600; time += 60) step(time);
		const moving = paths();
		expect(moving).not.toEqual(REST_PATHS);
		act(() => root?.render(<FlagMark variant="flag" paused />));
		expect(frames.size).toBe(0);
		step(1200);
		expect(paths()).toEqual(moving);
		act(() => root?.render(<FlagMark variant="flag" paused={false} />));
		step(2400);
		expect(paths()).toEqual(moving);
		step(2460);
		expect(paths()).not.toEqual(moving);
		act(() => root?.unmount());
		root = undefined;
		expect(frames.size).toBe(0);
		expect(vi.getTimerCount()).toBe(0);
	});

	it("stops when hidden and responds to a live reduced-motion preference", () => {
		const { frames, step, paths } = playback();
		let hidden = false;
		let preference = "full";
		vi.spyOn(document, "hidden", "get").mockImplementation(() => hidden);
		vi.spyOn(window, "getComputedStyle").mockReturnValue({ getPropertyValue: () => preference } as unknown as CSSStyleDeclaration);
		mount(<FlagMark variant="flag" paused={false} />);
		step(0);
		step(60);
		hidden = true;
		act(() => document.dispatchEvent(new Event("visibilitychange")));
		expect(frames.size).toBe(0);
		hidden = false;
		act(() => document.dispatchEvent(new Event("visibilitychange")));
		expect(frames.size).toBe(1);
		preference = "reduce";
		act(() => vi.advanceTimersByTime(400));
		expect(frames.size).toBe(0);
		expect(paths()).toEqual(REST_PATHS);
		preference = "full";
		act(() => vi.advanceTimersByTime(400));
		expect(frames.size).toBe(1);
	});

	it("restarts the single reveal and lets it pause", () => {
		mount();
		const before = Array.from(container.querySelectorAll("[data-motion-mark]"));
		act(() => Array.from(container.querySelectorAll("button")).find((button) => button.textContent === "Spill på nytt")?.click());
		const after = Array.from(container.querySelectorAll("[data-motion-mark]"));
		expect(after).toHaveLength(2);
		expect(after[0]).not.toBe(before[0]);
		act(() => Array.from(container.querySelectorAll("button")).find((button) => button.textContent === "Pause")?.click());
		expect(Array.from(container.querySelectorAll("button")).some((button) => button.textContent === "Fortsett")).toBe(true);
	});

	it("schedules no frames with the native CSS reduced-motion preference", () => {
		vi.spyOn(window, "getComputedStyle").mockReturnValue({ getPropertyValue: () => "reduce" } as unknown as CSSStyleDeclaration);
		const schedule = vi.spyOn(window, "requestAnimationFrame");
		mount(<FlagMark variant="flag" paused={false} />);
		expect(schedule).not.toHaveBeenCalled();
		expect(Array.from(container.querySelectorAll("[data-flag-contour]"), (path) => path.getAttribute("d"))).toEqual(REST_PATHS);
	});
});
