export const DESKTOP_ROUTE_TRANSITION = {
	durationMs: 700,
	easing: "cubic-bezier(.60,.30,.01,.99)",
	currentPageY: -300,
	incomingPageY: 300,
	incomingClipPath: "inset(90% 30% 15% 29%)",
	overlayOpacity: 0.4,
} as const;

export const MOBILE_ROUTE_TRANSITION = {
	durationMs: 400,
	easing: "cubic-bezier(.40,0,.20,1)",
} as const;

export type RouteTransitionMode = "desktop" | "mobile" | "none";

type RouteTransitionPreferences = {
	reducedMotion: boolean;
	narrowViewport: boolean;
	coarsePointer: boolean;
};

export function getRouteTransitionMode({
	reducedMotion,
	narrowViewport,
	coarsePointer,
}: RouteTransitionPreferences): RouteTransitionMode {
	if (reducedMotion) {
		return "none";
	}

	return narrowViewport || coarsePointer ? "mobile" : "desktop";
}

export function resolveInternalNavigation(
	href: string,
	currentHref: string,
): URL | null {
	if (!href || href.startsWith("#") || href.startsWith("//")) {
		return null;
	}

	const current = new URL(currentHref);
	const destination = new URL(href, current);

	if (
		destination.origin !== current.origin ||
		destination.pathname === current.pathname
	) {
		return null;
	}

	return destination;
}
