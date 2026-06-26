import { ArrowButton } from "@/components/ui/ArrowButton";
import { CpuArchitecture } from "@/components/ui/cpu-architecture";

const stats = [
	{ label: "Overvåk · brief · godkjenn · handle", unit: "steg", value: "4" },
	{ label: "Residens som standard (Sweden Central)", unit: "EØS", value: "EU" },
	{ label: "Risikofylte handlinger", unit: "godkjent", value: "HITL" },
];

export function TechnologySection() {
	return (
		<section
			className="relative isolate min-h-[100svh] overflow-hidden bg-background text-velion-j-text"
			id="produksjon"
		>
			<div
				aria-hidden="true"
				className="absolute inset-0 z-0 bg-[linear-gradient(90deg,rgba(23,23,23,0.026)_1px,transparent_1px),linear-gradient(180deg,rgba(23,23,23,0.022)_1px,transparent_1px),linear-gradient(180deg,#fbfaf8_0%,var(--background)_100%)] bg-[length:112px_112px,112px_112px,100%_100%]"
			/>

			<div
				aria-hidden="true"
				className="absolute inset-0 z-[1] bg-[linear-gradient(90deg,rgba(248,248,247,0.96)_0%,rgba(248,248,247,0.78)_26%,rgba(248,248,247,0.18)_52%,rgba(248,248,247,0.50)_100%),radial-gradient(circle_at_62%_46%,rgba(255,255,255,0.82),transparent_26%)]"
			/>

			<div className="pointer-events-none absolute inset-y-0 right-[-7vw] z-[1] grid w-[68vw] place-items-center opacity-[0.26] blur-[0.2px] saturate-0 max-[900px]:right-[-28vw] max-[900px]:w-[118vw]">
				<div className="w-[min(1050px,74vw)] scale-[1.72] text-velion-j-text/55 max-[900px]:scale-[1.28]">
					<CpuArchitecture
						className="h-auto w-full"
						height="100%"
						width="100%"
					/>
				</div>
			</div>

			<div
				aria-hidden="true"
				className="absolute inset-0 z-[2] bg-[linear-gradient(90deg,var(--background)_0%,rgba(248,248,247,0.94)_25%,rgba(248,248,247,0.54)_50%,rgba(248,248,247,0.18)_72%,rgba(248,248,247,0.38)_100%)]"
			/>

			<div className="relative z-[3] min-h-[100svh] px-[clamp(56px,5.55vw,208px)] py-[clamp(76px,6.4vw,118px)] max-[900px]:px-[clamp(24px,4vw,56px)]">
				<div className="grid min-h-[calc(100svh-clamp(152px,12.8vw,236px))] grid-rows-[auto_1fr_auto]">
					<div className="max-w-[520px]">
						<h1
							className="fade-out-top m-0 max-w-[500px] font-arbeit text-[clamp(3rem,3.9vw,4.7rem)] font-light leading-[1.02] tracking-[-0.055em] text-velion-j-text"
							data-fade-out-top
						>
							Overvåk, brief,
							<br />
							godkjenn, handle.
						</h1>

						<div
							className="fade-out-top mt-[clamp(28px,2.7vw,44px)] max-w-[440px]"
							data-fade-out-top
						>
							<p className="m-0 font-protokoll text-[clamp(1.02rem,1vw,1.18rem)] font-light leading-[1.55] text-velion-text-muted">
								Den nyttige Velion-sløyfen starter med endring: en kundemelding,
								en nettside som oppdateres, en konkurrent som beveger seg, et
								hull i en kobling eller en kunnskaps­konflikt. Velion gjør
								signalet til en brief, foreslår neste handling — og venter når
								godkjenning kreves.
							</p>
						</div>
					</div>

					<div />

					<div className="w-full max-w-[590px] pb-[clamp(18px,2.2vw,34px)]">
						<div className="grid grid-cols-3 gap-[clamp(18px,2vw,30px)] max-[700px]:grid-cols-1">
							{stats.map((stat) => (
								<div
									className="border-b border-velion-j-text/18 pb-[clamp(20px,2vw,28px)]"
									key={stat.label}
								>
									<strong className="flex items-baseline gap-3 whitespace-nowrap font-arbeit text-[clamp(2.35rem,2.9vw,4rem)] font-light leading-none tracking-[-0.055em] text-velion-j-text/82">
										{stat.value}
										<sup className="relative top-[-0.08em] font-protokoll text-[0.32em] font-light tracking-normal text-velion-j-text/64">
											{stat.unit}
										</sup>
									</strong>

									<span className="mt-5 block font-protokoll text-[clamp(0.98rem,0.95vw,1.12rem)] font-light leading-[1.25] text-velion-text-muted">
										{stat.label}
									</span>
								</div>
							))}
						</div>

						<div className="mt-[clamp(34px,3.5vw,56px)]">
							<ArrowButton href="#flyt" variant="coral">
								Se sløyfen i produktet
							</ArrowButton>
						</div>
					</div>
				</div>
			</div>
		</section>
	);
}

export default TechnologySection;