import Image from "next/image";
import Link from "next/link";
import { Footer } from "@/components/core/footer/Footer";
import { ArrowButton } from "@/components/ui/ArrowButton";
import { ContextExplorer } from "./ContextExplorer";
import { SharedContextHeader } from "./SharedContextHeader";

type SystemLogo = {
	label: string;
	src: string;
};

const familiarSystems: SystemLogo[] = [
	{ label: "Outlook", src: "/brand-logos/outlook.svg" },
	{ label: "Slack", src: "/brand-logos/slack.svg" },
	{ label: "Notion", src: "/brand-logos/notion.svg" },
	{ label: "Microsoft 365", src: "/brand-logos/microsoft-365.svg" },
	{ label: "SharePoint", src: "/brand-logos/sharepoint.svg" },
	{ label: "OneDrive", src: "/brand-logos/onedrive.svg" },
];

function LogoMark({ system }: { system: SystemLogo }) {
	return (
		<span
			aria-label={system.label}
			className="grid size-11 place-items-center rounded-[14px] border border-verevon-j-text/8 bg-white p-2.5 shadow-[0_8px_18px_rgba(23,23,23,0.04)]"
			role="img"
			title={system.label}
		>
			<Image
				alt=""
				aria-hidden="true"
				className="max-h-full max-w-full object-contain"
				height={24}
				src={system.src}
				style={{ height: "auto", width: "auto" }}
				width={24}
			/>
		</span>
	);
}

function HeroContextCard() {
	return (
		<div className="absolute bottom-[clamp(18px,2.4vw,34px)] left-[clamp(18px,2.4vw,34px)] right-[clamp(18px,2.4vw,34px)] rounded-[22px] border border-white/80 bg-white/94 p-[clamp(16px,2vw,24px)] shadow-[0_24px_64px_rgba(23,23,23,0.16)] backdrop-blur-md">
			<div className="flex items-start justify-between gap-5">
				<div>
					<p className="m-0 font-protokoll text-[0.65rem] font-medium uppercase tracking-[0.16em] text-verevon-j-text/64">
						Felles kontekst
					</p>
					<p className="m-0 mt-2 font-arbeit text-[clamp(1.1rem,1.55vw,1.45rem)] font-normal leading-[1.04] tracking-[-0.04em] text-verevon-j-text">
						Levering til Nora
					</p>
				</div>
				<span className="rounded-full bg-emerald-500/10 px-2.5 py-1 font-protokoll text-[0.68rem] font-medium text-emerald-800">
					Klar for neste steg
				</span>
			</div>

			<div className="mt-4 grid gap-2 border-t border-verevon-j-text/8 pt-3 font-protokoll text-[0.78rem] font-light text-verevon-j-text/62 sm:grid-cols-2">
				<span>Pågående kundesamtale</span>
				<span>Fire kilder samlet</span>
			</div>
		</div>
	);
}

export function SharedContextPage() {
	return (
		<div className="min-h-screen bg-background text-verevon-text [--verevon-edge:clamp(32px,5.55vw,208px)] [--verevon-page-pad:clamp(20px,4vw,56px)] [--verevon-section-vpad:clamp(80px,11vw,160px)]">
			<SharedContextHeader />

			<main>
				<section
					className="border-b border-verevon-j-text/8 px-[var(--verevon-edge)] pb-[clamp(72px,9vw,144px)] pt-[clamp(74px,10vw,148px)] max-[760px]:px-[var(--verevon-page-pad)]"
					id="top"
				>
					<div className="mx-auto grid max-w-[1680px] items-center gap-x-[clamp(44px,7.2vw,138px)] gap-y-12 xl:grid-cols-[minmax(0,0.9fr)_minmax(520px,1.1fr)]">
						<div className="max-w-[700px]">
							<p className="verevon-eyebrow !text-[#686867]">Plattform / Felles kontekst</p>
							<h1 className="verevon-display mt-7 max-w-[10ch] text-balance">
								Felles kontekst, før neste overlevering.
							</h1>
							<p className="verevon-body-lg mt-8 max-w-[52ch] text-pretty">
								Verevon samler historikken, kunnskapen og det som må skje videre — på tvers av arbeidsflatene dere allerede bruker.
							</p>
							<p className="mt-5 max-w-[54ch] font-protokoll text-[var(--text-body)] font-light leading-[1.55] text-verevon-j-text/60 text-pretty">
								Dere trenger ikke å bytte ut verdenen deres for å bruke Verevon. Dere får et felles utgangspunkt for samtalen, grunnlaget og neste steg.
							</p>

							<div className="mt-10 flex flex-wrap items-center gap-x-8 gap-y-5">
								<ArrowButton href="#eksempel">Se et eksempel</ArrowButton>
								<Link
									className="font-protokoll text-[0.92rem] font-light text-verevon-j-text/62 underline-offset-4 transition-colors hover:text-verevon-j-text hover:underline"
									href="/#kunnskap"
								>
									Se hvordan dette passer inn i arbeidsflyten
								</Link>
							</div>
						</div>

						<figure className="relative isolate min-h-[420px] overflow-hidden rounded-[30px] border border-verevon-j-text/8 bg-verevon-surface-soft shadow-[0_32px_100px_rgba(23,23,23,0.10)] sm:min-h-[520px]">
							<Image
								alt="Rolig kontormiljø med to kolleger ved et bord, som illustrerer arbeidet rundt en delt sak."
								className="object-cover object-[58%_center]"
								fill
								priority
								sizes="(max-width: 1280px) calc(100vw - 40px), 50vw"
								src="/shared-context-office.png"
							/>
							<HeroContextCard />
						</figure>
					</div>
				</section>

				<section
					aria-label="Systemer Verevon kan samle i arbeidskonteksten"
					className="border-b border-verevon-j-text/8 px-[var(--verevon-edge)] py-[clamp(34px,4vw,56px)] max-[760px]:px-[var(--verevon-page-pad)]"
				>
					<div className="mx-auto grid max-w-[1680px] items-center gap-7 lg:grid-cols-[minmax(250px,0.72fr)_minmax(0,1fr)]">
						<p className="m-0 max-w-[27ch] font-protokoll text-[0.9rem] font-light leading-[1.45] text-verevon-j-text/62">
							Arbeidet fortsetter i systemene dere kjenner.
						</p>
						<div className="flex flex-wrap items-center justify-start gap-2.5 lg:justify-end" aria-label="Kjente systemer">
							{familiarSystems.map((system) => (
								<LogoMark key={system.label} system={system} />
							))}
						</div>
					</div>
				</section>

				<ContextExplorer />

				<section
					className="border-b border-verevon-j-text/8 px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]"
					id="kilder"
				>
					<div className="mx-auto grid max-w-[1680px] items-center gap-x-[clamp(48px,8vw,150px)] gap-y-12 xl:grid-cols-[minmax(0,0.88fr)_minmax(480px,1.12fr)]">
						<div className="max-w-[580px]">
							<p className="verevon-eyebrow !text-[#686867]">Synlig grunnlag</p>
							<h2 className="verevon-h2 mt-6 max-w-[13ch] text-balance">
								Kontekst som forklarer seg selv.
							</h2>
							<p className="verevon-body-lg mt-7 max-w-[50ch] text-pretty">
								Når en samtale får en oppfølging, ser dere ikke bare resultatet. Dere ser hva som ligger bak, når det sist ble oppdatert og hva som fortsatt mangler.
							</p>
							<ul className="m-0 mt-9 grid list-none gap-0 border-t border-verevon-j-text/12 p-0">
								{[
									"Kilden som ble brukt",
									"Når informasjonen sist var oppdatert",
									"Hva som fremdeles trenger en vurdering",
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

						<figure className="relative min-h-[480px] overflow-hidden rounded-[30px] border border-verevon-j-text/8 bg-verevon-surface-soft shadow-[0_24px_80px_rgba(23,23,23,0.08)]">
							<Image
								alt="En person i et vindu med kodelinjer i refleksjonen, brukt som et redaksjonelt bilde på å kunne etterprøve informasjon."
								className="object-cover object-center"
								fill
								sizes="(max-width: 1280px) calc(100vw - 40px), 50vw"
								src="/aruc-launcher-after.jpg"
							/>
							<div className="absolute bottom-5 left-5 right-5 rounded-[20px] border border-white/82 bg-white/94 p-5 shadow-[0_18px_50px_rgba(23,23,23,0.16)] backdrop-blur-md sm:bottom-7 sm:left-7 sm:right-7">
								<div className="flex items-start justify-between gap-4">
									<div>
										<p className="m-0 font-protokoll text-[0.65rem] font-medium uppercase tracking-[0.16em] text-verevon-j-text/64">
											Kildegrunnlag
										</p>
										<p className="m-0 mt-1.5 font-arbeit text-[1.1rem] tracking-[-0.035em] text-verevon-j-text">
											Returpolicy 04
										</p>
									</div>
									<span className="rounded-full bg-verevon-surface-soft px-2.5 py-1 font-protokoll text-[0.68rem] text-verevon-j-text/62">
										Oppdatert i går
									</span>
								</div>
								<p className="m-0 mt-4 font-protokoll text-[0.85rem] font-light leading-[1.45] text-verevon-j-text/62">
									Grunnlaget er synlig før teamet tar neste beslutning.
								</p>
							</div>
						</figure>
					</div>
				</section>

				<section
					className="overflow-hidden bg-verevon-j-text px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] text-white max-[760px]:px-[var(--verevon-page-pad)]"
					id="overlevering"
				>
					<div className="mx-auto grid max-w-[1680px] items-center gap-x-[clamp(48px,8vw,150px)] gap-y-12 xl:grid-cols-[minmax(0,0.86fr)_minmax(480px,1.14fr)]">
						<div className="max-w-[580px]">
							<p className="font-protokoll text-[0.72rem] font-medium uppercase tracking-[0.28em] text-white/46">
								Overlevering
							</p>
							<h2 className="mt-6 max-w-[12ch] font-arbeit text-[clamp(2.45rem,4.1vw,5rem)] font-light leading-[0.94] tracking-[-0.06em] text-white text-balance">
								Håndoff uten å begynne på nytt.
							</h2>
							<p className="mt-7 max-w-[50ch] font-protokoll text-[var(--text-body-lg)] font-light leading-[1.52] text-white/64 text-pretty">
								Når noen andre tar over, trenger de ikke å grave seg bakover. De ser hva kunden har sagt, hva som ble brukt, hva som er gjort og hva som fortsatt trenger oppmerksomhet.
							</p>
							<ul className="m-0 mt-9 grid list-none border-t border-white/16 p-0">
								{[
									"Samtalen slik den står nå",
									"Kildene som bærer saken",
									"Det tydeligste neste steget",
								].map((item, index) => (
									<li className="grid grid-cols-[auto_1fr] gap-4 border-b border-white/16 py-4 font-protokoll text-[0.94rem] font-light text-white/66" key={item}>
										<span className="text-verevon-coral">0{index + 1}</span>
										<span>{item}</span>
									</li>
								))}
							</ul>
						</div>

						<figure className="relative min-h-[480px] overflow-hidden rounded-[30px] border border-white/12 bg-white/8">
							<Image
								alt="En person i en samtale, som illustrerer en menneskelig overlevering av arbeid."
								className="object-cover object-center"
								fill
								sizes="(max-width: 1280px) calc(100vw - 40px), 50vw"
								src="/man-talking-and-delegating.jpg"
							/>
							<div className="absolute bottom-5 left-5 right-5 rounded-[20px] border border-white/82 bg-white/94 p-5 text-verevon-j-text shadow-[0_18px_50px_rgba(0,0,0,0.28)] backdrop-blur-md sm:bottom-7 sm:left-7 sm:right-7">
								<p className="m-0 font-protokoll text-[0.65rem] font-medium uppercase tracking-[0.16em] text-verevon-j-text/64">
									Klar for overlevering
								</p>
								<p className="m-0 mt-1.5 font-arbeit text-[1.12rem] leading-[1.08] tracking-[-0.035em]">
									Neste person ser hva saken trenger.
								</p>
							</div>
						</figure>
					</div>
				</section>

				<section className="px-[var(--verevon-edge)] py-[clamp(92px,12vw,180px)] max-[760px]:px-[var(--verevon-page-pad)]">
					<div className="mx-auto grid max-w-[1680px] gap-10 border-t border-verevon-j-text/12 pt-[clamp(38px,5vw,72px)] xl:grid-cols-[minmax(0,1fr)_auto] xl:items-end">
						<div>
							<p className="verevon-eyebrow !text-[#686867]">Resultatet</p>
							<h2 className="verevon-h2 mt-6 max-w-[14ch] text-balance">
								Mindre leting. Færre avbrudd. Mer ro i arbeidet.
							</h2>
						</div>
						<div className="flex flex-wrap items-center gap-x-8 gap-y-5 xl:justify-end">
							<ArrowButton href="/produkt/arbeidsflyten">Se arbeidsflyten</ArrowButton>
							<ArrowButton href="/trust" variant="muted">
								Se tillitssenteret
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

export default SharedContextPage;
