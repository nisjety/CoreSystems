"use client";

import { useEffect, useState } from "react";

/**
 * PromptCycle — a single composer line that types through Verevon's jobs
 * (Lovart-inspired). Says "one prompt becomes all of this work" and leads
 * into the capability chapters. Reduced-motion: shows the first prompt static.
 */
const PROMPTS = [
	"Lag et svar på dagens uløste kundesamtale",
	"Bygg en arbeidsflyt fra hjelpesenteret",
	"Overvåk en konkurrent og skriv en brief",
	"Vis revisjonssporet for forrige handling",
];

export function PromptCycle() {
	const [text, setText] = useState(PROMPTS[0]);

	useEffect(() => {
		if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
			return;
		}

		let promptIndex = 0;
		let charIndex = PROMPTS[0].length;
		let deleting = true;
		let timer: ReturnType<typeof setTimeout>;

		const run = () => {
			const full = PROMPTS[promptIndex];

			if (deleting) {
				charIndex -= 1;
				setText(full.slice(0, charIndex));

				if (charIndex <= 0) {
					deleting = false;
					promptIndex = (promptIndex + 1) % PROMPTS.length;
					timer = setTimeout(run, 320);
					return;
				}

				timer = setTimeout(run, 22);
				return;
			}

			charIndex += 1;
			setText(PROMPTS[promptIndex].slice(0, charIndex));

			if (charIndex >= PROMPTS[promptIndex].length) {
				deleting = true;
				timer = setTimeout(run, 1700);
				return;
			}

			timer = setTimeout(run, 40);
		};

		timer = setTimeout(run, 1700);

		return () => clearTimeout(timer);
	}, []);

	return (
		<div className="inline-flex max-w-full items-center gap-3 rounded-full border border-verevon-j-text/12 bg-verevon-surface px-5 py-3 shadow-[var(--verevon-shadow-sm)]">
			<span
				aria-hidden="true"
				className="size-2 shrink-0 rounded-full bg-verevon-coral"
			/>
			<span className="truncate font-protokoll text-[clamp(0.92rem,1vw,1.08rem)] font-light text-verevon-j-text/80">
				{text}
				<span
					aria-hidden="true"
					className="ml-0.5 inline-block h-[1.05em] w-px translate-y-[0.18em] animate-pulse bg-verevon-coral"
				/>
			</span>
		</div>
	);
}

export default PromptCycle;
