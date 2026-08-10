import Image from "next/image";
import { Footer } from "@/components/core/footer/Footer";
import { ArrowButton } from "@/components/ui/ArrowButton";
import { ResponseTimeHeader } from "./ResponseTimeHeader";

const delaySteps = [
	{
		title: "Finn kilden",
		body: "Svaret finnes et sted, men ikke nødvendigvis der det første søket leter.",
	},
	{
		title: "Sjekk reglene",
		body: "En kilde uten kontekst kan være riktig i går og feil i dag.",
	},
	{
		title: "Formuler svaret",
		body: "Et presist svar krever at kilden og regelen faktisk stemmer sammen.",
	},
	{
		title: "Gjør neste steg riktig",
		body: "Svaret er ikke ferdig før det er koblet til det som skjer videre.",
	},
];

/**
 * ResponseTimePage — card 01's dedicated page ("Det er ikke svaret som tar tid").
 *
 * Second pass on this page: the first version leaned on source-traceability
 * ("kildespor") because the word "kilde" appears in card 01's body copy —
 * wrong move, that's card 02's argument. This version stays on card 01's
 * actual thesis (the four steps cost time, not the answer itself) and on
 * how an agent compresses them. The hero reuses the exact same file as
 * ProblemSection's hover-preview loop — what you preview on the card is
 * what you get here.
 */
export function ResponseTimePage() {
	return (
		<div className="min-h-screen bg-background text-verevon-text [--verevon-edge:clamp(32px,5.55vw,208px)] [--verevon-page-pad:clamp(20px,4vw,56px)] [--verevon-section-vpad:clamp(80px,11vw,160px)]">
			<ResponseTimeHeader />

			<main>
				<section
					className="border-b border-verevon-j-text/8 px-[var(--verevon-edge)] pb-[clamp(72px,9vw,144px)] pt-[clamp(74px,10vw,148px)] max-[760px]:px-[var(--verevon-page-pad)]"
					id="top"
				>
					<div className="mx-auto grid max-w-[1680px] items-center gap-x-[clamp(44px,7.2vw,138px)] gap-y-12 xl:grid-cols-[minmax(0,0.9fr)_minmax(520px,1.1fr)]">
						<div className="max-w-[700px]">
							<p className="verevon-eyebrow !text-[#686867]">Problemet / Tid</p>
							<h1 className="verevon-display mt-7 max-w-[16ch] text-balance">
								Svaret er raskt. Prosessen rundt det er ikke.
							</h1>
							<p className="verevon-body-lg mt-8 max-w-[48ch] text-pretty">
								Hvert steg er enkelt alene — finne kilden, sjekke reglene, formulere svaret, gjøre neste steg riktig. Sammen er de det som faktisk tar tid.
							</p>
							<p className="mt-5 max-w-[54ch] font-protokoll text-[var(--text-body)] font-light leading-[1.55] text-verevon-j-text/60 text-pretty">
								Verevon lar en agent gjøre alle fire samtidig og legge fram et ferdig utkast — i stedet for fire separate manuelle runder.
							</p>

							<div className="mt-10 flex flex-wrap items-center gap-x-8 gap-y-5">
								<ArrowButton href="#losning">Se hvordan</ArrowButton>
								<a
									className="font-protokoll text-[0.92rem] font-light text-verevon-j-text/62 underline-offset-4 transition-colors hover:text-verevon-j-text hover:underline focus-visible:text-verevon-j-text focus-visible:outline-none"
									href="/"
								>
									Tilbake til forsiden
								</a>
							</div>
						</div>

						<figure className="relative isolate min-h-[420px] overflow-hidden rounded-[30px] border border-verevon-j-text/8 bg-verevon-surface-soft shadow-[0_32px_100px_rgba(23,23,23,0.10)] sm:min-h-[520px]">
							<video
								autoPlay
								className="size-full object-cover"
								loop
								muted
								playsInline
								poster="/verevon-vibe/problem-delay-loop-poster.jpg"
								preload="metadata"
							>
								<source src="/verevon-vibe/problem-delay-loop.mp4" type="video/mp4" />
							</video>
						</figure>
					</div>
				</section>

				<section
					className="border-b border-verevon-j-text/8 px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]"
					id="problemet"
				>
					<div className="mx-auto grid max-w-[1680px] items-center gap-x-[clamp(48px,8vw,150px)] gap-y-12 xl:grid-cols-[minmax(0,0.88fr)_minmax(480px,1.12fr)]">
						<div className="max-w-[580px]">
							<p className="verevon-eyebrow !text-[#686867]">Problemet</p>
							<h2 className="verevon-h2 mt-6 max-w-[14ch] text-balance">
								Fire steg, hver med sin egen forsinkelse.
							</h2>
							<p className="verevon-body-lg mt-7 max-w-[48ch] text-pretty">
								AI kan formulere et svar raskt. Arbeidet rundt svaret — ikke selve svaret — er det som tar tid.
							</p>
							<ul className="m-0 mt-9 grid list-none gap-0 border-t border-verevon-j-text/12 p-0">
								{delaySteps.map((step, index) => (
									<li className="grid grid-cols-[auto_1fr] gap-4 border-b border-verevon-j-text/12 py-4 font-protokoll text-[0.94rem] font-light leading-[1.45] text-verevon-j-text/66" key={step.title}>
										<span className="font-medium text-verevon-coral">0{index + 1}</span>
										<div>
											<span className="block font-medium text-verevon-j-text/84">{step.title}</span>
											<span className="mt-1 block">{step.body}</span>
										</div>
									</li>
								))}
							</ul>
						</div>

						<figure className="relative min-h-[480px] overflow-hidden rounded-[30px] border border-verevon-j-text/8 bg-verevon-surface-soft shadow-[0_24px_80px_rgba(23,23,23,0.08)]">
							<Image
								alt="En mann sitter alene i et tomt, symmetrisk venterom og venter på at prosessen skal fortsette."
								className="object-cover object-center"
								fill
								sizes="(max-width: 1280px) calc(100vw - 40px), 50vw"
								src="/verevon-mood/svartid-waiting-room.jpg"
							/>
						</figure>
					</div>
				</section>

				<section
					className="border-b border-verevon-j-text/8 px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]"
					id="losning"
				>
					<div className="mx-auto grid max-w-[1680px] items-center gap-x-[clamp(48px,8vw,150px)] gap-y-12 xl:grid-cols-[minmax(0,0.88fr)_minmax(480px,1.12fr)]">
						<div className="max-w-[580px]">
							<p className="verevon-eyebrow !text-[#686867]">Løsningen</p>
							<h2 className="verevon-h2 mt-6 max-w-[13ch] text-balance">
								Agenten gjør stegene. Dere godkjenner resultatet.
							</h2>
							<p className="verevon-body-lg mt-7 max-w-[49ch] text-pretty">
								Verevon kobler kunnskap, regler og verktøy til én agent som gjør de fire stegene i sammenheng — søker, sjekker og formulerer et utkast før dere tar neste steg.
							</p>
							<p className="mt-6 max-w-[52ch] font-protokoll text-[var(--text-body)] font-light leading-[1.55] text-verevon-j-text/60 text-pretty">
								Det som tidligere krevde fire separate runder, blir ett utkast klart for vurdering. Tiden som gikk til å lete, går nå til å vurdere.
							</p>
						</div>

						<figure className="relative min-h-[480px] overflow-hidden rounded-[30px] border border-verevon-j-text/8 bg-verevon-surface-soft shadow-[0_24px_80px_rgba(23,23,23,0.08)]">
							<Image
								alt="En lang, symmetrisk korridor der flere personer venter hver for seg langs samme vegg — samme prosess, samme kø."
								className="object-cover object-center"
								fill
								sizes="(max-width: 1280px) calc(100vw - 40px), 50vw"
								src="/verevon-mood/svartid-corridor.jpg"
							/>
							<div className="absolute bottom-5 left-5 right-5 rounded-[20px] border border-white/82 bg-white/94 p-5 shadow-[0_18px_50px_rgba(23,23,23,0.16)] backdrop-blur-md sm:bottom-7 sm:left-7 sm:right-7">
								<p className="m-0 font-protokoll text-[0.65rem] font-medium uppercase tracking-[0.16em] text-verevon-j-text/64">
									Status i dette eksempelet
								</p>
								<p className="m-0 mt-1.5 font-arbeit text-[1.12rem] tracking-[-0.035em] text-verevon-j-text">
									Fire steg samlet til ett utkast.
								</p>
								<div className="mt-4 grid gap-2 border-t border-verevon-j-text/8 pt-3 font-protokoll text-[0.78rem] font-light text-verevon-j-text/62 sm:grid-cols-2">
									<span>Manuelt · fire runder</span>
									<span>Med Verevon · ett utkast</span>
								</div>
							</div>
						</figure>
					</div>
				</section>

				<section className="px-[var(--verevon-edge)] py-[clamp(92px,12vw,180px)] max-[760px]:px-[var(--verevon-page-pad)]">
					<div className="mx-auto grid max-w-[1680px] gap-10 border-t border-verevon-j-text/12 pt-[clamp(38px,5vw,72px)] xl:grid-cols-[minmax(0,1fr)_auto] xl:items-end">
						<div>
							<p className="verevon-eyebrow !text-[#686867]">Resten av arbeidet</p>
							<h2 className="verevon-h2 mt-6 max-w-[14ch] text-balance">
								Raskere er ikke det samme som ukontrollert.
							</h2>
							<p className="mt-6 max-w-[52ch] font-protokoll text-[var(--text-body)] font-light leading-[1.55] text-verevon-j-text/60 text-pretty">
								Et utkast er ikke en handling. Godkjenning og arbeidsflyten rundt svaret er de neste stegene.
							</p>
						</div>
						<div className="flex flex-wrap items-center gap-x-8 gap-y-5 xl:justify-end">
							<ArrowButton href="/trust">Se godkjenningsmodellen</ArrowButton>
							<ArrowButton href="/produkt/arbeidsflyten" variant="muted">
								Se arbeidsflyten
							</ArrowButton>
						</div>
					</div>
				</section>
			</main>

			<div className="relative overflow-clip bg-verevon-footer-bg">
				<Footer />
			</div>
		</div>
	);
}

export default ResponseTimePage;
