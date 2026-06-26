import { VisualPanel } from "./VisualPanel";

export function SensesSection() {
	return (
		<section
			className="grid min-h-[100svh] grid-cols-[minmax(320px,0.78fr)_minmax(0,0.92fr)] items-center gap-[clamp(48px,7vw,140px)] bg-background px-[clamp(56px,5.55vw,208px)] py-[clamp(96px,15vh,180px)] text-velion-j-text max-[1100px]:grid-cols-1 max-[1100px]:px-[clamp(24px,4vw,56px)]"
			id="kunnskap"
		>
			<VisualPanel
				className="w-full max-[1100px]:max-w-[720px]"
				label="Velion kildegraf og kunnskapsspor"
				variant="open"
			/>

			<div className="max-w-[760px] justify-self-end max-[1100px]:justify-self-start">
				<span
					aria-hidden="true"
					className="fade-out-top mb-8 block h-px w-[clamp(88px,9vw,168px)] bg-[color-mix(in_srgb,var(--velion-a-earth)_28%,var(--velion-border-strong))]"
					data-fade-out-top
				/>

				<h1
					className="fade-out-top m-0 max-w-[820px] font-arbeit text-[clamp(3.35rem,6.4vw,8.6rem)] font-light leading-[0.9] tracking-[-0.078em] text-velion-j-text"
					data-fade-out-top
				>
					Den forklarer hva den har lært.
				</h1>

				<div
					className="fade-out-top mt-[clamp(28px,3vw,46px)] grid max-w-[580px] gap-5"
					data-fade-out-top
				>
					<p className="m-0 font-protokoll text-[clamp(1.02rem,1.05vw,1.22rem)] font-light leading-[1.5] text-velion-text-muted">
						Velion kan koble til nettsider, dokumenter, innboks-historikk,
						integrasjoner og offentlige registre. Den henter dem ikke bare inn
						— den bygger en arbeidsminne mennesker kan inspisere, korrigere og
						bruke.
					</p>

					<p className="m-0 font-protokoll text-[clamp(1.02rem,1.05vw,1.22rem)] font-light leading-[1.5] text-velion-text-muted">
						Når arbeideren skriver et utkast eller foreslår en arbeidsflyt,
						holder grensesnittet kildesporet synlig: hentede tekstutdrag,
						policyer, koblingsstatus, sikkerhet og handlingen den ber om
						tillatelse til å utføre.
					</p>

					<p className="m-0 font-protokoll text-[clamp(1.02rem,1.05vw,1.22rem)] font-light leading-[1.5] text-velion-text-muted">
						Kunnskap er ikke gjemt bak modellen. Den blir en operativ flate.
					</p>
				</div>
			</div>
		</section>
	);
}

export default SensesSection;