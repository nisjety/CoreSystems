import { describe, expect, it } from "vitest";
import {
	DESKTOP_ROUTE_TRANSITION,
	getRouteTransitionMode,
	resolveInternalNavigation,
} from "./route-transition";

describe("route transition configuration", () => {
	it("matches the Coffee Tech desktop motion", () => {
		expect(DESKTOP_ROUTE_TRANSITION).toEqual({
			durationMs: 700,
			easing: "cubic-bezier(.60,.30,.01,.99)",
			currentPageY: -300,
			incomingPageY: 300,
			incomingClipPath: "inset(90% 30% 15% 29%)",
			overlayOpacity: 0.4,
		});
	});

	it("uses a crossfade for narrow or coarse-pointer devices", () => {
		expect(
			getRouteTransitionMode({
				reducedMotion: false,
				narrowViewport: true,
				coarsePointer: false,
			}),
		).toBe("mobile");
		expect(
			getRouteTransitionMode({
				reducedMotion: false,
				narrowViewport: false,
				coarsePointer: true,
			}),
		).toBe("mobile");
	});

	it("disables the custom transition when reduced motion is requested", () => {
		expect(
			getRouteTransitionMode({
				reducedMotion: true,
				narrowViewport: false,
				coarsePointer: false,
			}),
		).toBe("none");
	});
});

describe("resolveInternalNavigation", () => {
	const currentUrl = "http://localhost:3000/";

	it("accepts a different route on the current origin", () => {
		expect(
			resolveInternalNavigation("/trust", currentUrl)?.href,
		).toBe("http://localhost:3000/trust");
	});

	it("ignores hashes, external origins, and the current route", () => {
		expect(resolveInternalNavigation("#produkt", currentUrl)).toBeNull();
		expect(
			resolveInternalNavigation("https://example.com/trust", currentUrl),
		).toBeNull();
		expect(resolveInternalNavigation("/", currentUrl)).toBeNull();
		expect(
			resolveInternalNavigation("/?visning=kompakt", currentUrl),
		).toBeNull();
	});
});
