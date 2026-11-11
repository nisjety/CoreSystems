// /bot: the public information page for QuarryBot, the crawler behind Quarry.
//
// Static page, no runtime switch. Every claim on it has to match what is deployed:
// - The user agent is the D-W5-1 string (quarry_core::identity::HONEST_USER_AGENT). The Quarry
//   edge sends it only when QUARRY_EDGE__UNIFIED_UA is on, and that switch is off by default.
// - The robots gate defaults to observe (QUARRY_EDGE__CRAWL_ROBOTS, X-10). In observe it reads
//   and counts robots.txt but never denies or delays a fetch. Disallow and Crawl-delay are
//   applied only in enforce mode, which needs owner sign-off. Ad-hoc single-page scrapes run
//   without a robots check, so the copy scopes the robots claims to crawl, batch and listing.
// - The user agent names https://verevon.io/bot, while siteUrl is https://verevon.ai. This page
//   must also be reachable at verevon.io/bot, which is a DNS or redirect task outside this repo.
//   siteUrl is deliberately unchanged.
// Revisit the robots and user-agent copy when those defaults change.

import type { Metadata } from "next";
import Link from "next/link";
import { Footer } from "@/components/core/footer/Footer";
import { ArrowButton } from "@/components/ui/ArrowButton";
import { Eyebrow } from "@/components/ui/SectionHeading";
import { StatusBadge, type StatusLevel } from "@/components/ui/StatusBadge";

export const metadata: Metadata = {
	title: "QuarryBot — crawleren for sider en kunde ber om",
	description:
		"QuarryBot er Verevons crawler for sider en kunde ber den lese, ikke en søkeindeks. Her står user-agenten, hva den gjør med robots.txt og sperrer, og hvordan et nettsted melder seg av.",
	alternates: {
		canonical: "/bot",
	},
	openGraph: {
		title: "QuarryBot — user-agent, robots.txt og avmelding",
		description:
			"Hvilken user-agent QuarryBot bruker, hva den gjør med robots.txt og sperrer, og hvordan et nettsted melder seg av.",
		locale: "nb_NO",
		type: "website",
		url: "/bot",
	},
};

const CONTACT_EMAIL = "hei@verevon.ai";
const USER_AGENT = "QuarryBot/2.0 (+https://verevon.io/bot)";
const OPT_OUT_RULE = "User-agent: QuarryBot\nDisallow: /";

const robotsRows: { title: string; level: StatusLevel; body: string }[] = [
	{
		title: "Crawl og batch",
		level: "progress",
		body: "Disallow og Crawl-delay er det QuarryBot er bygget for å følge. Håndhevingen er ikke slått på som standard ennå. Til da registreres robots.txt-avgjørelsen, men hentingen av siden stanses ikke og bremses ikke.",
	},
	{
		title: "Kartlegging av et nettsted",
		level: "live",
		body: "Når robots.txt kan leses, tas adresser som den ikke tillater for QuarryBot, ikke med i listen.",
	},
	{
		title: "Sperrer og CAPTCHA-er",
		level: "live",
		body: "QuarryBot stanser ved en sperre. Den løser ikke CAPTCHA-er, og den bytter ikke identitet for å komme videre.",
	},
];

/** Plain header for this page. TrustHeader links to in-page anchors of the trust page, so it is not reused. */
function BotHeader() {
	return (
		<header className="sticky top-0 z-50 border-b border-verevon-j-text/8 bg-[color-mix(in_srgb,var(--background)_86%,transparent)] backdrop-blur-[14px]">
			<div className="mx-auto flex h-[72px] max-w-[1680px] items-center justify-between px-[var(--verevon-page-pad)]">
				<Link
					aria-label="Verevon forside"
					className="font-manrope text-base font-normal uppercase leading-none tracking-[0.44em] text-verevon-j-text"
					href="/"
				>
					Verevon
				</Link>

				<nav
					aria-label="QuarryBot-navigasjon"
					className="flex items-center gap-[clamp(20px,3vw,42px)] font-geist text-[0.78rem] font-medium uppercase tracking-[0.06em] text-verevon-j-text/60"
				>
					<Link className="transition-colors hover:text-verevon-j-text" href="/trust">
						Tillitssenter
					</Link>
					<Link className="transition-colors hover:text-verevon-j-text" href="/">
						Forsiden
					</Link>
				</nav>
			</div>
		</header>
	);
}

function IntroSection() {
	return (
		<section className="relative isolate overflow-hidden border-b border-verevon-j-text/8 bg-[linear-gradient(180deg,var(--verevon-trust-tint)_0%,var(--background)_60%)] px-[var(--verevon-edge)] pb-[clamp(72px,8vw,120px)] pt-[clamp(72px,10vh,128px)] max-[760px]:px-[var(--verevon-page-pad)]">
			<div className="mx-auto max-w-[1680px]">
				<Eyebrow marker>QuarryBot</Eyebrow>

				<h1 className="mt-7 max-w-[20ch] font-manrope text-[clamp(2.7rem,5.2vw,5.6rem)] font-light leading-[0.96] tracking-[-0.06em] text-verevon-j-text text-balance">
					En crawler for sider en kunde ber om. Ikke et søkeindeks.
				</h1>

				<p className="mt-8 max-w-[64ch] font-geist text-[var(--text-body-lg)] font-light leading-[1.5] text-verevon-text-muted text-pretty">
					QuarryBot er Verevons crawler. Den henter sider som en kunde ber Verevon lese,
					enten én side eller et nettsted kunden ber om å gå gjennom. Under står hvilken
					user-agent den bruker, hva den gjør med robots.txt og sperrer, og hvordan et
					nettsted melder seg av.
				</p>
			</div>
		</section>
	);
}

function UserAgentSection() {
	return (
		<section
			className="border-b border-verevon-j-text/8 bg-verevon-surface-soft/40 px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]"
			id="user-agent"
		>
			<div className="mx-auto max-w-[1680px]">
				<div className="max-w-[760px]">
					<Eyebrow>Identitet</Eyebrow>
					<h2 className="verevon-h2 mt-6 text-balance">User-agenten QuarryBot sender.</h2>
					<p className="verevon-body-lg mt-7 max-w-[62ch] text-pretty">
						QuarryBot identifiserer seg med denne strengen. Adressen i parentesen er den
						offentlige beskrivelsen av QuarryBot.
					</p>
				</div>

				<div className="mt-[clamp(40px,5vw,72px)] flex max-w-[760px] flex-col items-start gap-5">
					<StatusBadge level="progress" />
					<pre className="w-full overflow-x-auto rounded-[var(--radius-lg)] border border-verevon-j-text/12 bg-verevon-surface px-6 py-5 font-geist text-[0.95rem] font-light leading-[1.6] text-verevon-j-text">
						<code>{USER_AGENT}</code>
					</pre>
					<p className="verevon-body max-w-[62ch] text-pretty">
						Til innføringen er slått på i driften, kan forespørslene fortsatt bære en
						eldre streng.
					</p>
				</div>
			</div>
		</section>
	);
}

function RobotsSection() {
	return (
		<section
			className="border-b border-verevon-j-text/8 px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]"
			id="robots"
		>
			<div className="mx-auto max-w-[1680px]">
				<div className="max-w-[760px]">
					<Eyebrow marker>Atferd</Eyebrow>
					<h2 className="verevon-h2 mt-6 text-balance">
						Robots.txt og sperrer, slik det fungerer i dag.
					</h2>
					<p className="verevon-body-lg mt-7 max-w-[62ch] text-pretty">
						QuarryBot leser robots.txt for tokenene quarrybot og quarry. Har begge tokenene regler i filen,
						gjelder den strengeste avgjørelsen. Oversikten under viser hva som gjelder i
						dag.
					</p>
				</div>

				<ul className="mt-[clamp(40px,5vw,72px)] flex flex-col">
					{robotsRows.map((row) => (
						<li
							className="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-x-6 gap-y-3 border-t border-verevon-j-text/12 py-6 last:border-b sm:grid-cols-[minmax(220px,0.9fr)_auto_minmax(0,1.6fr)] sm:items-center"
							key={row.title}
						>
							<h3 className="verevon-h3 text-[clamp(1.2rem,1.3vw,1.6rem)]">{row.title}</h3>
							<div className="row-start-1 justify-self-end sm:row-start-auto sm:justify-self-start">
								<StatusBadge level={row.level} />
							</div>
							<p className="col-span-2 font-geist text-[var(--text-body-sm)] font-light leading-[1.5] text-verevon-text-muted text-pretty sm:col-span-1">
								{row.body}
							</p>
						</li>
					))}
				</ul>

				<p className="verevon-body mt-10 max-w-[62ch] text-pretty">
					Enkeltsider som en kunde ber om å lese, hentes i dag uten å sjekke robots.txt først.
					Har du sider som må holdes utenfor, bruk innlogging eller annen tilgangskontroll.
					robots.txt er en forespørsel til crawlere, ikke en tilgangskontroll.
				</p>
			</div>
		</section>
	);
}

function OptOutSection() {
	return (
		<section
			className="border-b border-verevon-j-text/8 bg-verevon-surface-soft/40 px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]"
			id="avmelding"
		>
			<div className="mx-auto max-w-[1680px]">
				<div className="max-w-[760px]">
					<Eyebrow>Avmelding</Eyebrow>
					<h2 className="verevon-h2 mt-6 text-balance">Slik melder et nettsted seg av.</h2>
					<p className="verevon-body-lg mt-7 max-w-[62ch] text-pretty">
						Legg til disse linjene i robots.txt på nettstedet ditt:
					</p>
				</div>

				<pre className="mt-10 max-w-[760px] overflow-x-auto rounded-[var(--radius-lg)] border border-verevon-j-text/12 bg-verevon-surface px-6 py-5 font-geist text-[0.95rem] font-light leading-[1.6] text-verevon-j-text">
					<code>{OPT_OUT_RULE}</code>
				</pre>

				<p className="verevon-body mt-7 max-w-[62ch] text-pretty">
					Regelen gjelder hele nettstedet. Vil du bare holde deler av det utenfor, bruk en
					Disallow-linje for de delene i stedet. Kartlegging av nettstedet tar allerede
					hensyn til regelen. Henting av sider stanses først når håndhevingen i oversikten
					ovenfor er slått på.
				</p>
			</div>
		</section>
	);
}

function ContactSection() {
	return (
		<section
			className="px-[var(--verevon-edge)] py-[var(--verevon-section-vpad)] max-[760px]:px-[var(--verevon-page-pad)]"
			id="kontakt-quarrybot"
		>
			<div className="mx-auto grid max-w-[1680px] items-end gap-x-[clamp(40px,5vw,96px)] gap-y-10 xl:grid-cols-[minmax(0,1fr)_auto]">
				<div>
					<Eyebrow marker>Kontakt</Eyebrow>
					<h2 className="verevon-h2 mt-6 max-w-[20ch] text-balance">Spørsmål om QuarryBot?</h2>
					<p className="verevon-body-lg mt-7 max-w-[58ch] text-pretty">
						Skriv til oss hvis du lurer på hvordan QuarryBot oppfører seg, eller hvis den har
						hentet en side den ikke burde.
					</p>
				</div>

				<div className="flex flex-col items-start gap-5 xl:items-end">
					<a
						className="font-manrope text-[clamp(1.3rem,1.6vw,1.9rem)] font-light tracking-[-0.04em] text-verevon-j-text underline-offset-[6px] transition-colors hover:text-verevon-coral-deep hover:underline"
						href={`mailto:${CONTACT_EMAIL}`}
					>
						{CONTACT_EMAIL}
					</a>
					<ArrowButton href={`mailto:${CONTACT_EMAIL}`} variant="coral">
						Skriv til oss
					</ArrowButton>
				</div>
			</div>
		</section>
	);
}

export default function BotPage() {
	return (
		<div className="min-h-screen bg-background text-verevon-text [--verevon-edge:clamp(56px,5.55vw,208px)] [--verevon-page-pad:clamp(24px,4vw,56px)] [--verevon-section-vpad:clamp(80px,11vh,140px)]">
			<BotHeader />
			<main className="bg-background text-verevon-j-text">
				<IntroSection />
				<UserAgentSection />
				<RobotsSection />
				<OptOutSection />
				<ContactSection />
			</main>

			<div className="relative overflow-clip bg-verevon-footer-bg">
				<Footer />
			</div>
		</div>
	);
}
