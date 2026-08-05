import Image from "next/image";
import Link from "next/link";
import { Footer } from "@/components/core/footer/Footer";
import { ArrowButton } from "@/components/ui/ArrowButton";
import { WorkflowExplorer } from "./WorkflowExplorer";
import { WorkflowHeader } from "./WorkflowHeader";

const evidence = [
	{
		title: "Kildegrunnlag",
		body: "Se hvilken samtale, policy eller systemopplysning som ligger bak forslaget.",
	},
	{
		title: "Regel og omfang",
		body: "Forslaget kan vurderes mot rammene dere allerede har satt.",
	},
	{
		title: "Menneskelig beslutning",
		body: "Godkjenn, revider eller gjør arbeidet selv fra den samme flaten.",
	},
	{
		title: "Synlig etterpå",
		body: "Beslutningen og endringen blir et tydelig utgangspunkt for neste runde.",
	},
];

function HeroWorkItem() {
	return (
		<figure className="relative min-h-[430px] overflow-hidden rounded-[30px] border border-verevon-j-text/10 bg-verevon-surface-soft shadow-[0_34px_110px_rgba(23,23,23,0.12)] sm:min-h-[530px]">
			<Image
				alt="Verevon-arbeidsflaten med en oppgave, kilder og et forslag som er klart for vurdering."
				className="object-cover object-[48%_center]"
				fill
				priority
				sizes="(max-width: 1280px) calc(100vw - 40px), 52vw"
				src="/verevon-product-shots/dashboard-live-prompt.png"
			/>
			<div className="absolute inset-x-0 top-0 h-28 bg-[linear-gradient(180deg,rgba(248,248,247,0.92),rgba(248,248,247,0))]" />
			<div className="absolute left-[clamp(16px,2.1vw,28px)] top-[clamp(16px,2.1vw,28px)] rounded-full border border-white/86 bg-white/92 px-3 py-2 font-protokoll text-[0.71rem] font-light text-verevon-j-text/64 shadow-[0_10px_24px_rgba(23,23,23,0.08)] backdrop-blur-md">
				<span className="mr-2 inline-block size-1.5 rounded-full bg-verevon-coral align-middle" />
				Pågående arbeid
			</div>
			<div className="absolute bottom-[clamp(16px,2.1vw,28px)] left-[clamp(16px,2.1vw,28px)] right-[clamp(16px,2.1vw,28px)] rounded-[21px] border border-white/84 bg-white/94 p-[clamp(16px,2vw,23px)] shadow-[0_18px_56px_rgba(23,23,23,0.14)] backdrop-blur-md">
				<div className="flex items-start justify-between gap-4">
					<div>
						<p className="m-0 font-protokoll text-[0.65rem] font-medium uppercase tracking-[0.15em] text-verevon-j-text/64">
							Forslag klart
						</p>
						<p className="m-0 mt-1.5 font-arbeit text-[clamp(1.05rem,1.5vw,1.35rem)] font-normal leading-[1.05] tracking-[-0.04em] text-verevon-j-text">
							Svar på forsinket levering
						</p>
					</div>
					<span className="shrink-0 rounded-full bg-verevon-coral-soft px-2.5 py-1 font-protokoll text-[0.67rem] text-verevon-a-earth">
						Kilder vedlagt
					</span>
				</div>
				<div className="mt-4 grid gap-2 border-t border-verevon-j-text/8 pt-3 font-protokoll text-[0.76rem] font-light text-verevon-j-text/62 sm:grid-cols-2">
					<span>Utkast venter på vurdering</span>
					<span>Manuelt alternativ tilgjengelig</span>
				</div>
			</div>
		</figure>
	);
}

export function WorkflowPage() {
	return (
		<div className="min-h-screen bg-background text-verevon-text [--verevon-edge:clamp(32px,5.55vw,208px)] [--verevon-page-pad:clamp(20px,4vw,56px)] [--verevon-section-vpad:clamp(80px,11vw,160px)]">
			<WorkflowHeader />

			<main>
				<section className="border-b border-verevon-j-text/8 px-[var(--verevon-edge)] pb-[clamp(76px,9vw,144px)] pt-[clamp(74px,10vw,148px)] max-[760px]:px-[var(--verevon-page-pad)]">
					<div className="mx-auto grid max-w-[1680px] items-center gap-x-[clamp(44px,7.2vw,138px)] gap-y-12 xl:grid-cols-[minmax(0,0.88fr)_minmax(520px,1.12fr)]">
						<div className="max-w-[690px]">
							<p className="verevon-eyebrow !text-[#686867]">Produkt / Arbeidsflyten</p>
							<h1 className="verevon-display mt-7 max-w-[11ch] text-balance">
								Fra signal til kontrollert handling.
							</h1>
							<p className="verevon-body-lg mt-8 max-w-[53ch] text-pretty">
								Verevon samler det som gjelder, forbereder et forslag og lar riktig person ta beslutningen før noe går videre.
							</p>
							<p className="mt-5 max-w-[53ch] font-protokoll text-[var(--text-body)] font-light leading-[1.55] text-verevon-j-text/60 text-pretty">
								Det er ikke en svart boks mellom spørsmålet og resultatet. Dere kan åpne grunnlaget, endre forslaget eller gjøre samme arbeid selv.
							</p>
							<div className="mt-10 flex flex-wrap items-center gap-x-8 gap-y-5">
								<ArrowButton href="#arbeidsflyt">Se arbeidsløkken</ArrowButton>
								<Link
									className="font-protokoll text-[0.92rem] font-light text-verevon-j-text/62 underline-offset-4 transition-colors hover:text-verevon-j-text hover:underline"
									href="/plattform/kontrollert-arbeid"
								>
									Se kontrollene rundt arbeidet
								</Link>
							</div>
						</div>

						<HeroWorkItem />
					</div>
				</section>

				<WorkflowExplorer />

				<section
					className="overflow-hidden bg-verevon-j-text px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] text-white max-[760px]:px-[var(--verevon-page-pad)]"
					id="bevis"
				>
					<div className="mx-auto max-w-[1680px]">
						<div className="grid gap-8 xl:grid-cols-[minmax(260px,0.72fr)_minmax(0,1fr)] xl:items-end">
							<div className="max-w-[390px]">
								<p className="font-protokoll text-[0.72rem] font-medium uppercase tracking-[0.28em] text-white/46">
									Se grunnlaget
								</p>
								<h2 className="mt-6 font-arbeit text-[clamp(2.5rem,4.5vw,5.6rem)] font-light leading-[0.92] tracking-[-0.065em] text-white text-balance">
									Arbeid som fortsatt kan forklares etterpå.
								</h2>
							</div>
							<p className="max-w-[58ch] font-protokoll text-[var(--text-body-lg)] font-light leading-[1.52] text-white/62 text-pretty xl:justify-self-end">
								Når teamet går videre, følger grunnlaget med. Det gjør det lettere å se hva som ble brukt, hva som ble bestemt og hva som er neste naturlige forbedring.
							</p>
						</div>

						<div className="mt-14 grid border-y border-white/16 sm:grid-cols-2 xl:grid-cols-4">
							{evidence.map((item, index) => (
								<article
									className="border-b border-white/16 py-8 sm:border-r sm:px-7 sm:py-10 sm:odd:border-r-0 xl:border-b-0 xl:border-r xl:px-8 xl:last:border-r-0"
									key={item.title}
								>
									<span className="font-protokoll text-[0.7rem] font-medium tracking-[0.16em] text-verevon-coral">
										0{index + 1}
									</span>
									<h3 className="mt-6 font-arbeit text-[1.45rem] font-normal leading-[1.05] tracking-[-0.04em] text-white">
										{item.title}
									</h3>
									<p className="mt-4 max-w-[27ch] font-protokoll text-[0.91rem] font-light leading-[1.5] text-white/58">
										{item.body}
									</p>
								</article>
							))}
						</div>
					</div>
				</section>

				<section
					className="border-b border-verevon-j-text/8 px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]"
					id="manuelt"
				>
					<div className="mx-auto grid max-w-[1680px] items-center gap-x-[clamp(48px,8vw,148px)] gap-y-12 xl:grid-cols-[minmax(0,0.9fr)_minmax(480px,1.1fr)]">
						<figure className="relative aspect-[1.15] overflow-hidden rounded-[30px] border border-verevon-j-text/9 bg-verevon-surface-soft shadow-[0_22px_76px_rgba(23,23,23,0.08)] xl:order-2">
							<Image
								alt="Verevon-chat med svarutkast og synlige retningslinjekilder som en person kan gjennomgå og endre."
								className="object-cover object-[52%_center]"
								fill
								sizes="(max-width: 1280px) calc(100vw - 40px), 50vw"
								src="/verevon-product-shots/chat-draft-sources-focus.jpg"
							/>
							<div className="absolute bottom-5 left-5 right-5 rounded-[18px] border border-white/84 bg-white/94 p-5 shadow-[0_18px_48px_rgba(23,23,23,0.15)] backdrop-blur-md sm:bottom-7 sm:left-7 sm:right-7">
								<p className="m-0 font-protokoll text-[0.65rem] font-medium uppercase tracking-[0.16em] text-verevon-j-text/64">
									Samme arbeid, synlig i UI
								</p>
								<p className="m-0 mt-1.5 font-arbeit text-[1.08rem] leading-[1.1] tracking-[-0.035em] text-verevon-j-text">
									Endre utkastet eller fullfør selv.
								</p>
							</div>
						</figure>

						<div className="max-w-[570px] xl:order-1">
							<p className="verevon-eyebrow !text-[#686867]">Mennesket kan alltid gjøre jobben</p>
							<h2 className="verevon-h2 mt-6 max-w-[13ch] text-balance">
								Autonomi skal ikke gjøre arbeidet uåpnelig.
							</h2>
							<p className="verevon-body-lg mt-7 max-w-[48ch] text-pretty">
								Alt Verevon forbereder, må kunne inspiseres og håndteres av et menneske i den samme arbeidsflaten.
							</p>
							<ul className="m-0 mt-9 grid list-none border-t border-verevon-j-text/12 p-0">
								{[
									"Åpne det Verevon brukte som grunnlag",
									"Endre utkastet før det sendes videre",
									"Gjennomfør oppgaven selv når det er riktig",
								].map((item, index) => (
									<li
										className="grid grid-cols-[auto_1fr] gap-4 border-b border-verevon-j-text/12 py-4 font-protokoll text-[0.94rem] font-light leading-[1.45] text-verevon-j-text/66"
										key={item}
									>
										<span className="font-medium text-verevon-coral">0{index + 1}</span>
										<span>{item}</span>
									</li>
								))}
							</ul>
						</div>
					</div>
				</section>

				<section className="px-[var(--verevon-edge)] py-[clamp(92px,12vw,180px)] max-[760px]:px-[var(--verevon-page-pad)]">
					<div className="mx-auto grid max-w-[1680px] gap-10 border-t border-verevon-j-text/12 pt-[clamp(38px,5vw,72px)] xl:grid-cols-[minmax(0,1fr)_auto] xl:items-end">
						<div>
							<p className="verevon-eyebrow !text-[#686867]">Neste bevis</p>
							<h2 className="verevon-h2 mt-6 max-w-[14ch] text-balance">
								Når arbeidet trenger kontroll, skal kontrollen være synlig.
							</h2>
						</div>
						<div className="flex flex-wrap items-center gap-x-8 gap-y-5 xl:justify-end">
							<ArrowButton href="/plattform/kontrollert-arbeid">
								Se kontrollert arbeid
							</ArrowButton>
							<ArrowButton href="/plattform/felles-kontekst" variant="muted">
								Se felles kontekst
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

export default WorkflowPage;
