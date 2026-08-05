"use client";

import { useEffect, useState } from "react";
import { VerevonMarkFilled } from "@/components/home/sections/VerevonMark";

type LoaderPhase = "intro" | "exit" | "done";

const EXIT_DELAY_MS = 650;
const DONE_DELAY_MS = 1360;

/**
 * Initial page loader using the filled Verevon mark. The mark path matches the
 * supplied `verevon-mark.svg` asset while avoiding an extra image request.
 */
export function PageLoader() {
	const [phase, setPhase] = useState<LoaderPhase>("intro");

	useEffect(() => {
		const reduceMotion = window.matchMedia(
			"(prefers-reduced-motion: reduce)",
		).matches;

		if (reduceMotion) {
			const skip = window.setTimeout(() => setPhase("done"), 0);
			return () => window.clearTimeout(skip);
		}

		const previousOverflow = document.body.style.overflow;
		document.body.style.overflow = "hidden";

		const toExit = window.setTimeout(() => setPhase("exit"), EXIT_DELAY_MS);
		const toDone = window.setTimeout(() => {
			setPhase("done");
			document.body.style.overflow = previousOverflow;
		}, DONE_DELAY_MS);

		return () => {
			window.clearTimeout(toExit);
			window.clearTimeout(toDone);
			document.body.style.overflow = previousOverflow;
		};
	}, []);

	if (phase === "done") {
		return null;
	}

	return (
		<div
			aria-hidden="true"
			className={[
				"verevon-page-loader",
				phase === "exit" ? "is-exiting" : "",
			]
				.filter(Boolean)
				.join(" ")}
		>
			<div className="verevon-page-loader__inner">
				<div className="verevon-page-loader__mark-frame">
					<VerevonMarkFilled className="verevon-page-loader__mark" />
				</div>
				<span className="verevon-page-loader__rule">
					<span />
				</span>
				<p className="verevon-page-loader__slogan">
					Verevon — Finn. Forstå. Få gjort.
				</p>
			</div>
		</div>
	);
}

export default PageLoader;
