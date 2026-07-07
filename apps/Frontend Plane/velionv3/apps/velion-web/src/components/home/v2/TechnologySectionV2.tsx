import { CpuArchitecture } from "@/components/ui/cpu-architecture";
import { Reveal } from "../sections/Reveal";

const stats = [
	{ label: "Overvåk · brief · godkjenn · handle", unit: "steg", value: "4" },
	{
		label: "Residens som standard (Sweden Central)",
		unit: "EØS",
		value: "EU",
	},
	{ label: "Risikofylte handlinger", unit: "godkjent", value: "HITL" },
];

/**
 * TechnologySectionV2 — production loop. Keeps the ambient CPU artwork but
 * drops the scroll-driven text masks; one calm reveal on enter.
 */
export function TechnologySectionV2() {
	return (
		<section
			className="relative isolate min-h-[100svh] overflow-hidden border-b border-velion-j-text/8 bg-background text-velion-j-text"
			id="produksjon"
		>
			<div
				aria-hidden="true"
				className="pointer-events-none absolute inset-y-0 right-[-7vw] z-0 grid w-[64vw] place-items-center opacity-[0.2] saturate-0 max-[900px]:right-[-28vw] max-[900px]:w-[118vw]"
			>
				<div className="w-[min(1000px,72vw)] scale-[1.6] text-velion-j-text/55 max-[900px]:scale-[1.2]">
					<CpuArchitecture
						className="h-auto w-full"
						height="100%"
						width="100%"
					/>
				</div>
			</div>

			<div
				aria-hidden="true"
				className="absolute inset-0 z-[1] bg-[linear-gradient(90deg,var(--background)_0%,rgba(248,248,247,0.82)_38%,rgba(248,248,247,0.32)_72%,rgba(248,248,247,0.5)_100%)]"
			/>

			<div className="relative z-[2] mx-auto flex min-h-[100svh] max-w-[1680px] flex-col justify-center px-[var(--velion-edge)] py-[var(--velion-section-vpad)] max-[760px]:px-[var(--velion-page-pad)]">
				<Reveal className="max-w-[540px]">
					<h2
						className="fade-out-top m-0 max-w-[500px] font-arbeit text-[clamp(2.6rem,4vw,4.7rem)] font-light leading-[1.02] tracking-[-0.05em] text-velion-j-text text-balance"
						data-fade-out-top
					>
						Overvåk, brief, godkjenn, handle.
					</h2>

					<p
						className="velion-body-lg fade-out-top mt-[clamp(28px,2.7vw,44px)] max-w-[440px]"
						data-fade-out-top
					>
						Den nyttige Velion-sløyfen starter med endring: en
						kundemelding, en nettside som oppdateres, en konkurrent
						som beveger seg eller en kunnskaps­konflikt. Velion gjør
						signalet til en brief, foreslår neste handling — og
						venter når godkjenning kreves.
					</p>
				</Reveal>

				<Reveal
					className="mt-[clamp(48px,6vw,88px)] w-full max-w-[640px]"
					delay={80}
				>
					<div className="grid grid-cols-3 gap-[clamp(18px,2vw,30px)] max-[700px]:grid-cols-1">
						{stats.map((stat) => (
							<div
								className="border-t border-velion-j-text/18 pt-[clamp(18px,2vw,26px)]"
								key={stat.label}
							>
								<strong className="flex items-baseline gap-2.5 whitespace-nowrap font-arbeit text-[clamp(2.2rem,2.8vw,3.6rem)] font-light leading-none tracking-[-0.05em] text-velion-j-text/82">
									{stat.value}
									<sup className="relative top-[-0.08em] font-protokoll text-[0.34em] font-light tracking-normal text-velion-j-text/64">
										{stat.unit}
									</sup>
								</strong>
								<span className="mt-4 block font-protokoll text-[var(--text-body-sm)] font-light leading-[1.25] text-velion-text-muted">
									{stat.label}
								</span>
							</div>
						))}
					</div>
				</Reveal>
			</div>
		</section>
	);
}

export default TechnologySectionV2;
