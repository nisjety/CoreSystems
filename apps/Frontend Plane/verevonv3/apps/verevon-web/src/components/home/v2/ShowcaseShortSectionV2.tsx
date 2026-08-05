"use client";

import { VerevonShortShowcaseStage } from "@/components/showcase/VerevonShortShowcase";
import { Reveal } from "../sections/Reveal";

export function ShowcaseShortSectionV2() {
	return (
		<section
			className="relative z-[70] isolate overflow-hidden border-y border-white/10 bg-[#2f3331] text-verevon-c-white"
			id="showcase-short"
		>
			<div
				aria-hidden="true"
				className="absolute inset-0 bg-[radial-gradient(circle_at_50%_36%,rgba(248,248,247,0.14),transparent_32%),radial-gradient(circle_at_42%_64%,rgba(185,43,139,0.12),transparent_25%),radial-gradient(circle_at_60%_66%,rgba(92,174,192,0.12),transparent_28%),linear-gradient(180deg,#969994_0%,#737873_48%,#4f5350_100%)]"
			/>
			<div
				aria-hidden="true"
				className="absolute inset-0 opacity-[0.16] [background-image:linear-gradient(rgba(255,255,255,0.08)_1px,transparent_1px),linear-gradient(90deg,rgba(255,255,255,0.06)_1px,transparent_1px)] [background-size:74px_74px]"
			/>

			<div className="relative z-[2] flex min-h-svh items-center justify-center overflow-hidden">
				<Reveal className="flex min-h-svh w-full items-center justify-center">
					<VerevonShortShowcaseStage
						ariaLabel="Kodet Verevon-produktfilm med 3D arbeidsflyt."
						variant="viewport"
					/>
				</Reveal>
			</div>
		</section>
	);
}

export default ShowcaseShortSectionV2;
