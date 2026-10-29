import { afterEach, describe, expect, it, vi } from "vitest";
import { createRibbonCopy } from "./ribbon-copy";

afterEach(() => vi.restoreAllMocks());

describe("fixed bitmap copy of the approved original", () => {
	it("uses source bitmap dimensions and clears before copying, regardless of presentation scale", () => {
		const source = document.createElement("canvas");
		const target = document.createElement("canvas");
		source.width = 710; source.height = 588;
		const context = { clearRect: vi.fn(), drawImage: vi.fn() };
		vi.spyOn(target, "getContext").mockImplementation((() => context) as unknown as typeof target.getContext);
		const measure = vi.spyOn(target, "getBoundingClientRect").mockImplementation(() => { throw new Error("Must not measure transformed presentation"); });
		const renderer = createRibbonCopy(target, source)!;
		renderer.draw(1500); renderer.draw(2000);
		expect([target.width, target.height]).toEqual([710, 588]);
		expect(context.drawImage).toHaveBeenLastCalledWith(source, 0, 0);
		expect(context.clearRect).toHaveBeenLastCalledWith(0, 0, 710, 588);
		expect(context.clearRect.mock.invocationCallOrder[0]).toBeLessThan(context.drawImage.mock.invocationCallOrder[0]);
		expect(measure).not.toHaveBeenCalled();
		renderer.dispose(); renderer.draw(2200);
		expect(context.drawImage).toHaveBeenCalledTimes(2);
	});

	it("returns a static fallback when a 2D surface is unavailable", () => {
		const target = document.createElement("canvas");
		vi.spyOn(target, "getContext").mockReturnValue(null);
		expect(createRibbonCopy(target, document.createElement("canvas"))).toBeNull();
	});
});
