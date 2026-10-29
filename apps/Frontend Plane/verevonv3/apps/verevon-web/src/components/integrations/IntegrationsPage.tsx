import Image from "next/image";
import { ArrowButton } from "@/components/ui/ArrowButton";
import { Footer } from "@/components/core/footer/Footer";
import { IntegrationDirectory } from "./IntegrationDirectory";
import { IntegrationsHeader } from "./IntegrationsHeader";

const controlPoints = [
	["01", "Tilgang på deres premisser", "Velg hvilke arbeidsflater som kan brukes, og avgrens hva som er relevant."],
	["02", "Kilden følger svaret", "Gjør grunnlaget synlig, slik at teamet kan vurdere informasjonen før de handler."],
	["03", "Kontrollen kan endres", "Juster eller trekk tilbake tilganger når oppgaver, team og behov endrer seg."],
];

export function IntegrationsPage() {
	return (
		<div className="min-h-screen bg-background text-verevon-text [--verevon-edge:clamp(32px,5.55vw,208px)] [--verevon-page-pad:clamp(20px,4vw,56px)] [--verevon-section-vpad:clamp(80px,11vw,160px)]">
			<IntegrationsHeader />
			<main>
				<section className="px-[var(--verevon-edge)] pb-[clamp(74px,9vw,132px)] pt-[clamp(46px,5vw,76px)] max-[760px]:px-[var(--verevon-page-pad)]" id="top">
					<div className="mx-auto max-w-[1060px] text-center">
						<h1 className="verevon-display mx-auto max-w-[18ch] text-balance">Koble systemene dere allerede bruker.</h1>
						<p className="verevon-body-lg mx-auto mt-5 max-w-[55ch] text-pretty">Verevon samler avtalte kilder til felles kontekst — med tilgangen dere velger.</p>
						<ArrowButton className="mt-6" href="#katalog">Se integrasjonene</ArrowButton>
					</div>
					<div className="relative mx-auto mt-[clamp(32px,3.8vw,56px)] aspect-[3.35] max-w-[1680px] overflow-hidden rounded-[24px] border border-verevon-j-text/7 bg-[#efefed] max-[760px]:aspect-[1.05]">
						<Image
							alt="To kollegaer som arbeider sammen ved et bord"
							className="object-cover object-[58%_52%]"
							fill
							priority
							sizes="(min-width: 1800px) 1680px, (min-width: 760px) 89vw, 100vw"
							src="/shared-context-office.png"
						/>
					</div>
				</section>

				<IntegrationDirectory />

				<section className="border-y border-verevon-j-text/8 bg-[#f6f7f6] px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]" id="kontroll">
					<div className="mx-auto max-w-[1680px]">
						<div className="max-w-[650px]">
							<p className="verevon-eyebrow !text-[#686867]">Kontrollert tilkobling</p>
							<h2 className="verevon-h2 mt-5 max-w-[13ch] text-balance">Kontekst, uten å miste oversikten.</h2>
						</div>
						<div className="mt-14 grid border-t border-verevon-j-text/12 md:grid-cols-3">
							{controlPoints.map(([number, title, detail]) => (
								<div className="border-b border-verevon-j-text/12 py-7 md:border-b-0 md:border-r md:px-7 md:first:pl-0 md:last:border-r-0 md:last:pr-0" key={number}>
									<p className="m-0 font-geist text-[0.68rem] font-medium uppercase tracking-[0.15em] text-verevon-coral">{number}</p>
									<h3 className="m-0 mt-8 font-manrope text-[1.45rem] font-normal leading-[1.04] tracking-[-0.045em]">{title}</h3>
									<p className="m-0 mt-4 font-geist text-[0.88rem] font-light leading-[1.5] text-verevon-j-text/62">{detail}</p>
								</div>
							))}
						</div>
					</div>
				</section>

				<section className="overflow-hidden bg-verevon-j-text px-[var(--verevon-edge)] py-[clamp(88px,11vw,154px)] text-white max-[760px]:px-[var(--verevon-page-pad)]">
					<div className="mx-auto grid max-w-[1680px] gap-12 xl:grid-cols-[minmax(0,1fr)_auto] xl:items-end">
						<div>
							<p className="m-0 font-geist text-[0.68rem] font-medium uppercase tracking-[0.18em] text-white/44">Finn en god start</p>
							<h2 className="mt-6 max-w-[12ch] font-manrope text-[clamp(2.5rem,5vw,6rem)] font-light leading-[0.9] tracking-[-0.07em] text-balance">Begynn med systemene dere kjenner best.</h2>
						</div>
						<ArrowButton href="/#kontakt" variant="light">Snakk med oss</ArrowButton>
					</div>
				</section>
			</main>
			<Footer />
		</div>
	);
}

export default IntegrationsPage;
