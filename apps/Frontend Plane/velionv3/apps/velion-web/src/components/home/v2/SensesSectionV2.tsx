import { VisualPanel } from "@/components/home/sections/VisualPanel";
import { Reveal } from "../sections/Reveal";

/**
 * SensesSectionV2 — knowledge as an operating surface. Calm two-column layout,
 * no scroll-driven text masks; one reveal on enter.
 */
export function SensesSectionV2() {
	return (
		<section
			className="grid min-h-[100svh] grid-cols-[minmax(320px,0.78fr)_minmax(0,0.92fr)] items-center gap-[clamp(48px,7vw,140px)] border-b border-velion-j-text/8 bg-background px-[var(--velion-edge)] py-[var(--velion-section-vpad)] text-velion-j-text max-[1100px]:grid-cols-1 max-[760px]:px-[var(--velion-page-pad)]"
			id="kunnskap"
		>
			<Reveal className="w-full max-[1100px]:max-w-[720px]">
				<VisualPanel
					className="w-full"
					label="Velion kildegraf og kunnskapsspor"
					variant="open"
				/>
			</Reveal>

			<Reveal className="max-w-[760px] justify-self-end max-[1100px]:justify-self-start">
				<span
					aria-hidden="true"
					className="mb-8 block h-px w-[clamp(88px,9vw,168px)] bg-[color-mix(in_srgb,var(--velion-a-earth)_28%,var(--velion-border-strong))]"
				/>

				<h2
					className="fade-out-top m-0 max-w-[820px] font-arbeit text-[clamp(2.6rem,5vw,6rem)] font-light leading-[0.94] tracking-[-0.06em] text-velion-j-text text-balance"
					data-fade-out-top
				>
					Den forklarer hva den har lært.
				</h2>

				<div
					className="fade-out-top mt-[clamp(28px,3vw,46px)] grid max-w-[580px] gap-5"
					data-fade-out-top
				>
					<p className="velion-body-lg m-0">
						Velion kan koble til nettsider, dokumenter,
						innboks-historikk, integrasjoner og offentlige registre.
						Den henter dem ikke bare inn — den bygger en
						arbeidsminne mennesker kan inspisere, korrigere og
						bruke.
					</p>
					<p className="velion-body-lg m-0">
						Når arbeideren skriver et utkast, holder grensesnittet
						kildesporet synlig: hentede tekstutdrag, policyer,
						koblingsstatus og handlingen den ber om tillatelse til å
						utføre.
					</p>
					<p className="velion-body-lg m-0">
						Kunnskap er ikke gjemt bak modellen. Den blir en
						operativ flate.
					</p>
				</div>
			</Reveal>
		</section>
	);
}

export default SensesSectionV2;
