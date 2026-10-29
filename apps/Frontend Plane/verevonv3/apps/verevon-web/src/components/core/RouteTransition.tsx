"use client";

import { useCallback, useEffect, useLayoutEffect, useRef } from "react";
import { usePathname } from "next/navigation";
import {
	DESKTOP_ROUTE_TRANSITION,
	getRouteTransitionMode,
	MOBILE_ROUTE_TRANSITION,
	resolveInternalNavigation,
	type RouteTransitionMode,
} from "./route-transition";

const PAGE_SHELL_SELECTOR = "[data-verevon-page-shell]";
const TRANSITION_STATE_ATTRIBUTE = "data-verevon-route-transition";
const TRANSITION_MODE_ATTRIBUTE = "data-verevon-route-transition-mode";
const NAVIGATION_WATCHDOG_MS = 5_000;

type TransitionSnapshot = {
	frame: HTMLDivElement;
	mode: Exclude<RouteTransitionMode, "none">;
	previousBodyOverflow: string;
	timeoutId: number;
	frameId: number;
	innerFrameId: number;
};

function isModifiedClick(event: MouseEvent) {
	return (
		event.button !== 0 ||
		event.metaKey ||
		event.ctrlKey ||
		event.shiftKey ||
		event.altKey
	);
}

function getBrowserTransitionMode(): RouteTransitionMode {
	return getRouteTransitionMode({
		reducedMotion: window.matchMedia(
			"(prefers-reduced-motion: reduce)",
		).matches,
		narrowViewport: window.matchMedia("(max-width: 767px)").matches,
		coarsePointer: window.matchMedia("(pointer: coarse)").matches,
	});
}

function syncClonedMedia(source: HTMLElement, clone: HTMLElement) {
	const sourceVideos = source.querySelectorAll("video");
	const clonedVideos = clone.querySelectorAll("video");

	sourceVideos.forEach((video, index) => {
		const clonedVideo = clonedVideos.item(index);

		if (!clonedVideo) {
			return;
		}

		clonedVideo.muted = video.muted;
		clonedVideo.playbackRate = video.playbackRate;

		try {
			clonedVideo.currentTime = video.currentTime;
		} catch {
			// Some streams do not expose a seekable range while being cloned.
		}

		void clonedVideo.play().catch(() => undefined);
	});
}

function createPageSnapshot(
	shell: HTMLElement,
	mode: Exclude<RouteTransitionMode, "none">,
): TransitionSnapshot {
	const frame = document.createElement("div");
	const pageViewport = document.createElement("div");
	const pageClone = shell.cloneNode(true) as HTMLElement;
	const shade = document.createElement("div");
	const scrollY = window.scrollY;

	frame.className = [
		"verevon-route-transition",
		`verevon-route-transition--${mode}`,
	].join(" ");
	frame.setAttribute("aria-hidden", "true");
	frame.inert = true;

	pageViewport.className = "verevon-route-transition__viewport";
	pageClone.classList.add("verevon-route-transition__page");
	pageClone.removeAttribute("data-verevon-page-shell");
	pageClone.classList.remove("verevon-page-shell");
	pageClone.style.top = `${-scrollY}px`;
	shade.className = "verevon-route-transition__shade";

	pageViewport.append(pageClone, shade);
	frame.append(pageViewport);
	document.body.append(frame);
	syncClonedMedia(shell, pageClone);

	const previousBodyOverflow = document.body.style.overflow;

	return {
		frame,
		mode,
		previousBodyOverflow,
		timeoutId: 0,
		frameId: 0,
		innerFrameId: 0,
	};
}

export function RouteTransition() {
	const pathname = usePathname();
	const previousPathname = useRef(pathname);
	const snapshotRef = useRef<TransitionSnapshot | null>(null);

	const cleanupTransition = useCallback(() => {
		const snapshot = snapshotRef.current;

		if (!snapshot) {
			return;
		}

		window.clearTimeout(snapshot.timeoutId);
		window.cancelAnimationFrame(snapshot.frameId);
		window.cancelAnimationFrame(snapshot.innerFrameId);
		snapshot.frame.remove();
		document.body.style.overflow = snapshot.previousBodyOverflow;
		document.documentElement.removeAttribute(TRANSITION_STATE_ATTRIBUTE);
		document.documentElement.removeAttribute(TRANSITION_MODE_ATTRIBUTE);
		snapshotRef.current = null;
	}, []);

	const prepareTransition = useCallback(() => {
		if (snapshotRef.current) {
			return false;
		}

		const mode = getBrowserTransitionMode();
		const shell = document.querySelector<HTMLElement>(PAGE_SHELL_SELECTOR);

		if (mode === "none" || !shell) {
			return false;
		}

		const snapshot = createPageSnapshot(shell, mode);
		snapshotRef.current = snapshot;
		snapshot.timeoutId = window.setTimeout(
			cleanupTransition,
			NAVIGATION_WATCHDOG_MS,
		);
		return true;
	}, [cleanupTransition]);

	useEffect(() => {
		const handleClick = (event: MouseEvent) => {
			if (isModifiedClick(event)) {
				return;
			}

			const target = event.target;
			const anchor =
				target instanceof Element
					? target.closest<HTMLAnchorElement>("a[href]")
					: null;
			const href = anchor?.getAttribute("href");
			const destination =
				href &&
				anchor?.target !== "_blank" &&
				!anchor?.hasAttribute("download")
					? resolveInternalNavigation(href, window.location.href)
					: null;

			if (!destination) {
				return;
			}

			prepareTransition();
		};

		const handlePopState = () => {
			prepareTransition();
		};

		window.addEventListener("click", handleClick, true);
		window.addEventListener("popstate", handlePopState, true);

		return () => {
			window.removeEventListener("click", handleClick, true);
			window.removeEventListener("popstate", handlePopState, true);
		};
	}, [prepareTransition]);

	useLayoutEffect(() => {
		if (pathname === previousPathname.current) {
			return;
		}

		previousPathname.current = pathname;
		const snapshot = snapshotRef.current;

		if (!snapshot) {
			return;
		}

		const duration =
			snapshot.mode === "desktop"
				? DESKTOP_ROUTE_TRANSITION.durationMs
				: MOBILE_ROUTE_TRANSITION.durationMs;

		window.clearTimeout(snapshot.timeoutId);
		document.body.style.overflow = "hidden";
		document.documentElement.setAttribute(
			TRANSITION_MODE_ATTRIBUTE,
			snapshot.mode,
		);
		document.documentElement.setAttribute(
			TRANSITION_STATE_ATTRIBUTE,
			"pending",
		);
		window.scrollTo(0, 0);

		snapshot.frameId = window.requestAnimationFrame(() => {
			snapshot.innerFrameId = window.requestAnimationFrame(() => {
				document.documentElement.setAttribute(
					TRANSITION_STATE_ATTRIBUTE,
					"running",
				);
				snapshot.frame.classList.add("is-running");
			});
		});

		snapshot.timeoutId = window.setTimeout(
			cleanupTransition,
			duration + 80,
		);

		return () => {
			window.cancelAnimationFrame(snapshot.frameId);
			window.cancelAnimationFrame(snapshot.innerFrameId);
		};
	}, [cleanupTransition, pathname]);

	useEffect(() => cleanupTransition, [cleanupTransition]);

	return null;
}

export default RouteTransition;
