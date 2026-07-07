import { Reveal } from "../sections/Reveal";

// Placeholder slots — replace each with a real customer/partner logo later.
const SLOTS = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"];

/**
 * ProofSectionV2 — social proof beat: testimonial + "trusted by" logo row.
 *
 * Honesty gate: we do NOT fabricate customer quotes or logos. Both are clearly
 * marked placeholders to be filled with a real design-partner quote and real
 * logos once available.
 */
export function ProofSectionV2() {
	return (
		<section
			aria-label="Tidlige partnere"
			className="border-b border-velion-j-text/8 bg-velion-surface-soft/40 px-[var(--velion-edge)] py-[var(--velion-section-vpad)] text-velion-j-text max-[760px]:px-[var(--velion-page-pad)]"
			id="kunder"
		>
			<div className="mx-auto max-w-[1680px]">
				<Reveal className="mx-auto flex max-w-[1000px] flex-col items-center text-center">
					<p className="velion-eyebrow">Tidlige partnere</p>

					<blockquote className="mt-9 m-0">
						<p className="font-arbeit text-[clamp(1.8rem,3vw,3.4rem)] font-light leading-[1.15] tracking-[-0.03em] text-velion-j-text text-balance">
							«Plassholder — et sitat fra en design-partner settes
							inn her: om hvordan Velion gjorde kundearbeidet
							raskere, med kildene synlige og mennesket i
							kontroll.»
						</p>
					</blockquote>

					<figcaption className="mt-8 flex items-center gap-3 font-protokoll text-[var(--text-body-sm)] font-light text-velion-text-muted">
						<span className="h-px w-8 bg-velion-coral/60" />
						Design-partner · rolle
						<span className="text-velion-text-muted/60">
							(plassholder)
						</span>
					</figcaption>
				</Reveal>

				<Reveal className="mt-[clamp(56px,7vw,104px)]">
					<div className="flex flex-wrap items-baseline justify-between gap-x-8 gap-y-2 border-t border-velion-j-text/10 pt-9">
						<p className="velion-eyebrow">
							Betrodd av norske virksomheter
						</p>
						<p className="font-protokoll text-[0.82rem] font-light text-velion-text-muted/70">
							Plassholder — ekte kunde- og partnerlogoer settes
							inn her
						</p>
					</div>

					<ul className="mt-8 grid grid-cols-2 items-center gap-x-8 gap-y-6 sm:grid-cols-3 lg:grid-cols-6">
						{SLOTS.map((slot) => (
							<li
								className="flex h-14 items-center justify-center rounded-[12px] border border-dashed border-velion-j-text/16 bg-velion-surface/60 font-protokoll text-[0.78rem] font-medium uppercase tracking-[0.14em] text-velion-j-text/26"
								key={slot}
							>
								Logo
							</li>
						))}
					</ul>
				</Reveal>
			</div>
		</section>
	);
}

export default ProofSectionV2;
