"use client";

import { useEffect, useState } from "react";

/**
 * Defaults to reduced motion until the browser preference is available. This
 * prevents autoplaying media from briefly starting for people who opt out.
 */
export function usePrefersReducedMotion() {
	const [prefersReducedMotion, setPrefersReducedMotion] = useState(true);

	useEffect(() => {
		const mediaQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
		const updatePreference = () => setPrefersReducedMotion(mediaQuery.matches);

		updatePreference();
		mediaQuery.addEventListener("change", updatePreference);

		return () => mediaQuery.removeEventListener("change", updatePreference);
	}, []);

	return prefersReducedMotion;
}
