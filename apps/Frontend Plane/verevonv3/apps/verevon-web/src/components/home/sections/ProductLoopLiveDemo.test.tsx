import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { LiveDemoMobileCard, LoopLiveDemo } from "./ProductLoopLiveDemo";
import { PRODUCT_LOOP_LIVE_DEMO } from "./product-loop-live-demo";
import { PRODUCT_LOOP_SETTLE_EVENT } from "./product-loop-progress";

// The live demo's iframe host. The frame's contentWindow is replaced by a fake, so messages can be
// sent from it (event.source === contentWindow) and the host's outgoing messages can be checked.
// jsdom has no <dialog> methods, so a small polyfill stands in for showModal and close.

type FakeWindow = {
	location: { replace: ReturnType<typeof vi.fn> };
	postMessage: ReturnType<typeof vi.fn>;
};

let container: HTMLElement;
let root: Root | null = null;
let fake: FakeWindow;
let restoreFullscreen = () => {};

beforeAll(() => {
	(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
	const dialogPrototype = (
		typeof HTMLDialogElement === "function" ? HTMLDialogElement.prototype : HTMLElement.prototype
	) as HTMLElement;
	if (typeof (dialogPrototype as { showModal?: unknown }).showModal !== "function") {
		Object.defineProperties(dialogPrototype, {
			close: {
				configurable: true,
				value(this: HTMLElement) {
					if (!this.hasAttribute("open")) return;
					this.removeAttribute("open");
					this.dispatchEvent(new Event("close"));
				},
			},
			showModal: {
				configurable: true,
				value(this: HTMLElement) {
					this.setAttribute("open", "");
				},
			},
		});
	}
});

afterEach(() => {
	if (root) act(() => root?.unmount());
	root = null;
	container?.remove();
	restoreFullscreen();
	restoreFullscreen = () => {};
	vi.useRealTimers();
	vi.restoreAllMocks();
	Reflect.deleteProperty(navigator, "connection");
	Reflect.deleteProperty(document, "hidden");
	document.documentElement.style.overflow = "";
});

const installFullscreenApi = () => {
	const documentFullscreenDescriptor = Object.getOwnPropertyDescriptor(
		document,
		"fullscreenElement",
	);
	const documentExitDescriptor = Object.getOwnPropertyDescriptor(
		document,
		"exitFullscreen",
	);
	const elementRequestDescriptor = Object.getOwnPropertyDescriptor(
		HTMLElement.prototype,
		"requestFullscreen",
	);
	let fullscreenElement: Element | null = null;
	const requestFullscreen = vi.fn(() => {
		fullscreenElement = document.querySelector("[data-product-loop-live]");
		document.dispatchEvent(new Event("fullscreenchange"));
		return Promise.resolve();
	});
	const exitFullscreen = vi.fn(() => {
		fullscreenElement = null;
		document.dispatchEvent(new Event("fullscreenchange"));
		return Promise.resolve();
	});

	Object.defineProperty(document, "fullscreenElement", {
		configurable: true,
		get: () => fullscreenElement,
	});
	Object.defineProperty(document, "exitFullscreen", {
		configurable: true,
		value: exitFullscreen,
	});
	Object.defineProperty(HTMLElement.prototype, "requestFullscreen", {
		configurable: true,
		value: requestFullscreen,
	});

	restoreFullscreen = () => {
		if (documentFullscreenDescriptor) {
			Object.defineProperty(document, "fullscreenElement", documentFullscreenDescriptor);
		} else {
			Reflect.deleteProperty(document, "fullscreenElement");
		}
		if (documentExitDescriptor) {
			Object.defineProperty(document, "exitFullscreen", documentExitDescriptor);
		} else {
			Reflect.deleteProperty(document, "exitFullscreen");
		}
		if (elementRequestDescriptor) {
			Object.defineProperty(HTMLElement.prototype, "requestFullscreen", elementRequestDescriptor);
		} else {
			Reflect.deleteProperty(HTMLElement.prototype, "requestFullscreen");
		}
	};

	return { exitFullscreen, requestFullscreen };
};

const mount = (element: ReturnType<typeof createElement>) => {
	container = document.createElement("div");
	document.body.append(container);
	root = createRoot(container);
	act(() => root?.render(element));
};

// A desktop stage: the layer the timeline toggles, around the live demo.
const mountStage = (settled = "true") =>
	mount(
		createElement(
			"div",
			{ "aria-hidden": "false", "data-product-loop-layer": "3", "data-product-loop-settled": settled },
			createElement(LoopLiveDemo, { live: PRODUCT_LOOP_LIVE_DEMO }),
		),
	);

const frameOf = () => container.querySelector<HTMLIFrameElement>("iframe");
const attachFake = () => {
	const frame = frameOf();
	if (frame && !Object.prototype.hasOwnProperty.call(frame, "contentWindow")) {
		Object.defineProperty(frame, "contentWindow", { configurable: true, value: fake });
	}
};
// Let timers run (the browser-idle fallback is 200 ms), then attach the fake to the mounted frame.
const settle = async (ms = 260) => {
	await act(async () => {
		await new Promise((resolve) => setTimeout(resolve, ms));
	});
	attachFake();
};
const demoSays = (type: string, extra: Record<string, unknown> = {}, source: unknown = fake) => {
	const event = new Event("message");
	Object.defineProperty(event, "data", { value: { source: "verevon-demo", type, v: 1, ...extra } });
	Object.defineProperty(event, "source", { value: source });
	act(() => {
		window.dispatchEvent(event);
	});
};
const posted = () => fake.postMessage.mock.calls.map(([message]) => (message as { type: string }).type);
const shield = () => container.querySelector<HTMLButtonElement>('button[aria-label^="Klikk for å prøve"]');
const fullscreenButton = () =>
	container.querySelector<HTMLButtonElement>('[aria-label="Vis dashboard i fullskjerm"]');
const buttonText = (text: string) =>
	[...container.querySelectorAll<HTMLButtonElement>("button")].find((item) => item.textContent?.trim() === text) ?? null;
const pressButton = (element: HTMLElement | null) => {
	expect(element, "button").not.toBeNull();
	act(() => element?.click());
};
const setLayer = async (name: string, value: string) => {
	const layer = container.querySelector<HTMLElement>("[data-product-loop-layer]");
	await act(async () => {
		layer?.setAttribute(name, value);
	});
};
// The stage carries the state on its root; the mobile dialog carries it on the <dialog>.
const state = () =>
	(container.querySelector<HTMLElement>("[data-product-loop-live]") ?? container.querySelector<HTMLElement>("dialog"))?.dataset.state;

// A stage that has loaded and been activated by a click on the shield.
const activeStage = async () => {
	mountStage();
	await settle();
	demoSays("ready");
	fake.postMessage.mockClear();
	pressButton(shield());
	return frameOf()!;
};

describe("LoopLiveDemo on the stage", () => {
	it("waits for the stage to near, then mounts a sandboxed frame out of the tab order", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		mountStage();
		expect(frameOf()).toBeNull();
		await settle();
		const frame = frameOf()!;
		expect(state()).toBe("loading");
		expect(frame.getAttribute("src")).toBe("/product-demo/index.html");
		expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
		expect(frame.getAttribute("title")).toBe("Verevon-demo: dashbordet med fiktive data for Fjordform AS");
		expect(frame.getAttribute("tabindex")).toBe("-1");
		expect(frame.getAttribute("aria-hidden")).toBe("true");
		expect(container.textContent).toContain("Laster demoen …");
	});

	it("shows the shield once the demo is ready, and activates it on click", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		const settleEvent = vi.fn();
		window.addEventListener(PRODUCT_LOOP_SETTLE_EVENT, settleEvent);
		mountStage();
		await settle();
		const frame = frameOf()!;
		const focus = vi.spyOn(frame, "focus");
		demoSays("ready");
		expect(state()).toBe("ready");
		expect(shield()?.getAttribute("aria-label")).toContain("Klikk for å prøve");
		expect(posted()).toEqual(["resume"]);

		pressButton(shield());
		expect(state()).toBe("active");
		expect(posted()).toEqual(["resume", "activate"]);
		expect(fake.postMessage).toHaveBeenLastCalledWith({ source: "verevon-demo", type: "activate", v: 1 }, "*");
		expect(frame.hasAttribute("tabindex")).toBe(false);
		expect(frame.hasAttribute("aria-hidden")).toBe(false);
		expect(focus).toHaveBeenCalledWith({ preventScroll: true });
		expect(settleEvent).toHaveBeenCalledTimes(1);
		expect(settleEvent.mock.calls[0]?.[0]).toMatchObject({ detail: { reducedMotion: false } });
		expect(buttonText("Avslutt demo")).not.toBeNull();
		window.removeEventListener(PRODUCT_LOOP_SETTLE_EVENT, settleEvent);
	});

	it("opens and closes the dashboard in native full screen from the demo toolbar", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		const fullscreen = installFullscreenApi();
		mountStage();
		await settle();
		demoSays("ready");

		const open = fullscreenButton();
		expect(open).not.toBeNull();
		expect(open?.getAttribute("title")).toBe("Vis dashboard i fullskjerm");
		pressButton(open);
		expect(fullscreen.requestFullscreen).toHaveBeenCalledTimes(1);
		expect(container.querySelector<HTMLElement>("[data-product-loop-live]")?.dataset.fullscreen).toBe("true");

		const close = container.querySelector<HTMLButtonElement>(
			'[aria-label="Avslutt fullskjerm"]',
		);
		expect(close).not.toBeNull();
		pressButton(close);
		expect(fullscreen.exitFullscreen).toHaveBeenCalledTimes(1);
		expect(container.querySelector<HTMLElement>("[data-product-loop-live]")?.dataset.fullscreen).toBe("false");
	});

	it("counts only messages from the frame's own window with the envelope", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		mountStage();
		await settle();
		demoSays("ready", {}, { someone: "else" });
		demoSays("ready", { source: "other" });
		expect(state()).toBe("loading");
		demoSays("ready");
		expect(state()).toBe("ready");
	});

	it("fails a stage whose demo reports an error while it loads", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		mountStage();
		await settle();
		demoSays("error", { message: "uncaught error" });
		expect(state()).toBe("failed");
		expect(frameOf()).toBeNull();
	});

	it("keeps a loaded demo running when it reports an error, since the demo reports benign notices too", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		mountStage();
		await settle();
		demoSays("ready");
		demoSays("error", { message: "ResizeObserver loop completed with undelivered notifications." });
		expect(state()).toBe("ready");
		expect(frameOf()).not.toBeNull();
	});

	it("loads the stage poster lazily, so a poster that is not shown is not fetched", () => {
		mountStage();
		const poster = container.querySelector<HTMLImageElement>("img");
		expect(poster?.getAttribute("loading")).toBe("lazy");
		expect(poster?.getAttribute("decoding")).toBe("async");
	});

	it("ends an active demo on Escape from the demo, and returns focus to the shield", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		await activeStage();
		demoSays("escape");
		expect(state()).toBe("ready");
		expect(posted()).toEqual(["activate", "deactivate"]);
		expect(document.activeElement).toBe(shield());
	});

	it("ends an active demo with the Avslutt demo button", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		await activeStage();
		pressButton(buttonText("Avslutt demo"));
		expect(state()).toBe("ready");
		expect(posted()).toEqual(["activate", "deactivate"]);
		expect(document.activeElement).toBe(shield());
	});

	it("ends an active demo on a pointer press outside the stage, without moving focus", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		await activeStage();
		act(() => {
			document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
		});
		expect(state()).toBe("ready");
		expect(posted()).toEqual(["activate", "deactivate"]);
	});

	it("ends an active demo when focus moves outside the stage", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		await activeStage();
		const outside = document.createElement("button");
		document.body.append(outside);
		act(() => outside.focus());
		expect(state()).toBe("ready");
		outside.remove();
	});

	it("ends an active demo on Escape pressed on the page", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		await activeStage();
		act(() => {
			document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
		});
		expect(state()).toBe("ready");
	});

	it("ends an active demo when the stage leaves its settled window, or is hidden", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		await activeStage();
		await setLayer("data-product-loop-settled", "false");
		expect(state()).toBe("ready");
		expect(posted()).toEqual(["activate", "deactivate"]);

		await setLayer("data-product-loop-settled", "true");
		pressButton(shield());
		await setLayer("aria-hidden", "true");
		expect(state()).toBe("ready");
		expect(posted().slice(-2)).toEqual(["deactivate", "pause"]);
	});

	it("pauses while the tab is hidden and resumes when it is shown again", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		mountStage();
		await settle();
		demoSays("ready");
		fake.postMessage.mockClear();
		Object.defineProperty(document, "hidden", { configurable: true, value: true });
		act(() => {
			document.dispatchEvent(new Event("visibilitychange"));
		});
		Object.defineProperty(document, "hidden", { configurable: true, value: false });
		act(() => {
			document.dispatchEvent(new Event("visibilitychange"));
		});
		expect(posted()).toEqual(["pause", "resume"]);
	});

	it("starts the demo afresh when the visitor comes back to the stage", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		mountStage();
		await settle();
		demoSays("ready");
		await setLayer("aria-hidden", "true");
		fake.postMessage.mockClear();
		await setLayer("aria-hidden", "false");
		expect(posted()).toEqual(["reset", "resume"]);
	});

	it("covers the reload with the poster, and fails only when the demo never answers", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		mountStage();
		await settle();
		demoSays("ready");
		vi.useFakeTimers();
		pressButton(buttonText("Start på nytt"));
		expect(posted().slice(-1)).toEqual(["reset"]);
		expect(state()).toBe("loading");
		expect(container.textContent).toContain("Laster demoen …");
		act(() => {
			vi.advanceTimersByTime(3000);
		});
		// The site never navigates the frame: the demo reloads itself, and a second navigation would cut that short.
		expect(fake.location.replace).not.toHaveBeenCalled();
		expect(state()).toBe("loading");
		act(() => {
			vi.advanceTimersByTime(12000);
		});
		expect(state()).toBe("failed");
	});

	it("announces the restart, and moves focus to the shield, when the demo answers the reset", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		mountStage();
		await settle();
		demoSays("ready");
		pressButton(buttonText("Start på nytt"));
		demoSays("ready");
		expect(fake.location.replace).not.toHaveBeenCalled();
		expect(container.querySelector('[role="status"]')?.textContent).toBe("Demoen er startet på nytt.");
		expect(document.activeElement).toBe(shield());
	});

	it("fails after 15 seconds without ready, and a retry mounts a fresh frame", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		vi.useFakeTimers();
		mountStage();
		act(() => {
			vi.advanceTimersByTime(260);
		});
		attachFake();
		expect(state()).toBe("loading");
		act(() => {
			vi.advanceTimersByTime(15000);
		});
		expect(state()).toBe("failed");
		expect(frameOf()).toBeNull();
		expect(container.textContent).toContain("Demoen kunne ikke lastes akkurat nå.");
		pressButton(buttonText("Prøv igjen"));
		expect(state()).toBe("loading");
		expect(frameOf()).not.toBeNull();
	});

	it("defers loading when the visitor saves data, until a button is pressed", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		Object.defineProperty(navigator, "connection", { configurable: true, value: { saveData: true } });
		mountStage();
		await settle();
		expect(state()).toBe("deferred");
		expect(frameOf()).toBeNull();
		pressButton(buttonText("Last inn demoen"));
		expect(state()).toBe("loading");
		expect(frameOf()).not.toBeNull();
	});
});

describe("LiveDemoMobileCard", () => {
	const dialogOf = () => container.querySelector<HTMLDialogElement>("dialog")!;

	it("loads the phone poster lazily as well, and mounts no frame before «Åpne demoen»", () => {
		mount(createElement(LiveDemoMobileCard, { live: PRODUCT_LOOP_LIVE_DEMO }));
		const poster = container.querySelector<HTMLImageElement>("img");
		expect(poster?.getAttribute("loading")).toBe("lazy");
		expect(poster?.getAttribute("decoding")).toBe("async");
		expect(frameOf()).toBeNull();
	});

	it("shows a poster card and mounts the frame only after «Åpne demoen»", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		mount(createElement(LiveDemoMobileCard, { live: PRODUCT_LOOP_LIVE_DEMO }));
		await settle(0);
		expect(frameOf()).toBeNull();
		expect(container.textContent).toContain("Demo · fiktive data");
		pressButton(buttonText("Åpne demoen"));
		expect(dialogOf().hasAttribute("open")).toBe(true);
		expect(frameOf()).not.toBeNull();
		attachFake();
		demoSays("ready");
		expect(state()).toBe("active");
		expect(posted()).toEqual(["activate", "resume"]);
	});

	it("keeps the same frame when the dialog closes, and returns focus to the trigger", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		mount(createElement(LiveDemoMobileCard, { live: PRODUCT_LOOP_LIVE_DEMO }));
		await settle(0);
		const trigger = buttonText("Åpne demoen")!;
		pressButton(trigger);
		attachFake();
		demoSays("ready");
		const frame = frameOf();
		expect(document.documentElement.style.overflow).toBe("hidden");
		fake.postMessage.mockClear();
		pressButton(buttonText("Lukk"));
		expect(dialogOf().hasAttribute("open")).toBe(false);
		expect(frameOf()).toBe(frame);
		expect(document.documentElement.style.overflow).toBe("");
		expect(document.activeElement).toBe(trigger);
		expect(posted()).toEqual(["deactivate", "pause"]);
	});

	it("resets the demo when the dialog is opened again, and activates the reloaded demo", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		mount(createElement(LiveDemoMobileCard, { live: PRODUCT_LOOP_LIVE_DEMO }));
		await settle(0);
		pressButton(buttonText("Åpne demoen"));
		attachFake();
		demoSays("ready");
		pressButton(buttonText("Lukk"));
		fake.postMessage.mockClear();
		pressButton(buttonText("Åpne demoen"));
		// The poster covers the reload until the reloaded demo reports ready; only then does it activate.
		expect(posted()).toEqual(["reset", "resume"]);
		expect(state()).toBe("loading");
		demoSays("ready");
		expect(state()).toBe("active");
		expect(posted()).toEqual(["reset", "resume", "activate", "resume"]);
	});

	it("closes on Escape from the demo", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		mount(createElement(LiveDemoMobileCard, { live: PRODUCT_LOOP_LIVE_DEMO }));
		await settle(0);
		pressButton(buttonText("Åpne demoen"));
		attachFake();
		demoSays("ready");
		demoSays("escape");
		expect(dialogOf().hasAttribute("open")).toBe(false);
	});

	it("keeps Tab inside the open dialog", async () => {
		fake = { location: { replace: vi.fn() }, postMessage: vi.fn() };
		mount(createElement(LiveDemoMobileCard, { live: PRODUCT_LOOP_LIVE_DEMO }));
		await settle(0);
		pressButton(buttonText("Åpne demoen"));
		attachFake();
		demoSays("ready");
		// Shift+Tab from the first control wraps to the last focusable item, the frame itself.
		const frame = frameOf()!;
		const focusFrame = vi.spyOn(frame, "focus");
		const first = buttonText("Start på nytt")!;
		act(() => first.focus());
		act(() => {
			dialogOf().dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "Tab", shiftKey: true }));
		});
		expect(focusFrame).toHaveBeenCalledWith({ preventScroll: true });
	});
});
