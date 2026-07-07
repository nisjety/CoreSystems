import Image from "next/image";
import { Reveal } from "../sections/Reveal";

// All three are LIVE per VELION.md. Deliberately NOT claiming eval/cost
// dashboards (roadmap) — honesty gate.
const points = [
	{
		title: "Observerbare kjøringer",
		body: "Hver agent-kjøring vises steg for steg i Agent Run Console — du ser hva som skjedde, ikke bare resultatet.",
	},
	{
		title: "Godkjenning per handling",
		body: "Risikofylte verktøy stopper ved en Godkjenn/Avvis-port før de utføres.",
	},
	{
		title: "«Brukt av AI?»-revisjon",
		body: "Et revisjonsspor på tvers av planene viser hvilke data AI-en rørte, per kategori.",
	},
];

export function ObservabilitySectionV2() {
	return (
		<section
			className="grid min-h-[88svh] grid-cols-[minmax(0,0.92fr)_minmax(320px,0.78fr)] items-center gap-[clamp(48px,7vw,140px)] border-b border-velion-j-text/8 bg-background px-[var(--velion-edge)] py-[var(--velion-section-vpad)] text-velion-j-text max-[1100px]:grid-cols-1 max-[760px]:px-[var(--velion-page-pad)]"
			id="observasjon"
		>
			<Reveal className="max-w-[620px]">
				<p className="velion-eyebrow">Fra pilot til produksjon</p>

				<h2
					className="fade-out-top mt-6 m-0 max-w-[16ch] font-arbeit text-[clamp(2.6rem,4.4vw,5rem)] font-light leading-[0.96] tracking-[-0.06em] text-velion-j-text text-balance"
					data-fade-out-top
				>
					Observert, godkjent, revidert.
				</h2>

				<p className="velion-body-lg mt-7 max-w-[48ch]">
					Å få agenter i produksjon — og holde dem etterrettelige — er
					den vanskelige delen. Velion gir deg sporet: hver kjøring
					synlig, hver risikofylt handling godkjent, hver databruk
					revidert.
				</p>

				<ul className="mt-10 grid gap-7">
					{points.map((point) => (
						<li
							className="grid grid-cols-[auto_minmax(0,1fr)] items-start gap-4 border-t border-velion-j-text/12 pt-6"
							key={point.title}
						>
							<span
								aria-hidden="true"
								className="mt-1.5 size-2 rounded-full bg-velion-coral"
							/>
							<div>
								<h3 className="velion-h3 text-[clamp(1.2rem,1.4vw,1.6rem)]">
									{point.title}
								</h3>
								<p className="velion-body mt-2.5 max-w-[44ch]">
									{point.body}
								</p>
							</div>
						</li>
					))}
				</ul>
			</Reveal>

			<Reveal className="w-full max-[1100px]:max-w-[640px]">
				<figure className="relative m-0 aspect-[3/4] overflow-hidden rounded-[6px] border border-velion-j-text/10 bg-white/55 shadow-[var(--velion-shadow-md)]">
					<Image
						alt="Agent Run Console viser en agent-kjøring steg for steg."
						className="object-cover object-top opacity-[0.92]"
						fill
						sizes="(max-width: 1100px) 90vw, 38vw"
						src="/velion-product-shots/chat-agent-steps.png"
					/>
					<div className="absolute inset-0 bg-[linear-gradient(180deg,transparent_64%,rgba(248,248,247,0.28))]" />
					<figcaption className="absolute bottom-4 left-4 font-protokoll text-[10px] uppercase leading-none tracking-[0.16em] text-velion-j-text/45">
						Agent Run Console · live
					</figcaption>
				</figure>
			</Reveal>
		</section>
	);
}

export default ObservabilitySectionV2;
