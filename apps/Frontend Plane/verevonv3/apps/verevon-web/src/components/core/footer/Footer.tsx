import Link from "next/link";

// Section links use absolute "/#anchor" form so they resolve from any route
// (e.g. the /trust page), not just the homepage.
const footerColumns = [
	[
		{ href: "/#produkt", label: "Produkt" },
		{ href: "/#plattform", label: "Plattform" },
		{ href: "/integrasjoner", label: "Integrasjoner" },
		{ href: "/#kunnskap", label: "Kunnskap" },
	],
	[
		{ href: "/#flyt", label: "Arbeidssløyfen" },
	],
	[
		{ href: "/trust", label: "Tillitssenter ↗" },
		{ href: "/trust#dataflyt", label: "Dataflyt og underleverandører ↗" },
		{ href: "/trust#kontroller", label: "Sikkerhet ↗" },
	],
	[
		{ href: "/#kontakt", label: "Kontakt" },
		{ href: "/#kontakt", label: "Snakk med oss" },
	],
];

const contactLinks = [
	{ href: "mailto:hei@verevon.ai", label: "hei@verevon.ai" },
	{ href: "/trust", label: "Sikkerhet og tillit" },
];

export function Footer() {
	const currentYear = new Date().getFullYear();

	return (
		<footer
			className="relative overflow-hidden bg-verevon-footer-bg font-manrope text-verevon-j-text lg:min-h-[73svh]"
			id="kontakt"
			data-footer-parallax=""
		>
			<div
				className="px-[var(--verevon-edge)] pb-0 pt-[clamp(24px,2.5vw,40px)] max-[760px]:px-[var(--verevon-page-pad)] lg:h-full"
				data-footer-parallax-media=""
			>
				<div className="grid min-h-[calc(73svh-112px)] grid-cols-[minmax(180px,0.9fr)_minmax(0,3.1fr)] gap-[clamp(48px,6vw,112px)] max-[900px]:min-h-0 max-[900px]:grid-cols-1">
					<div className="relative top-10 grid content-start gap-8">
						<Link
							aria-label="Verevon hjem"
							className="verevon-wordmark text-[clamp(1.6rem,2vw,2.25rem)] font-normal uppercase leading-none tracking-[0.08em] text-verevon-j-text"
							href="/#top"
						>
							VEREVON
						</Link>

						<p className="m-0 max-w-[290px] text-[clamp(0.9rem,0.95vw,1rem)] font-normal leading-[1.55] tracking-[0.005em] text-verevon-j-text/74">
							Én arbeidsflate på tvers av systemene dere bruker
							— med kunnskap, agentarbeid og kontroll samlet.
						</p>

						<div className="flex items-center gap-2.5 text-[0.72rem] font-medium uppercase tracking-[0.055em] text-verevon-j-text/64">
							<svg aria-hidden="true" className="h-4 w-6 shrink-0 shadow-sm" viewBox="0 0 22 16">
								<rect fill="#BA0C2F" height="16" width="22" />
								<rect fill="#FFFFFF" height="4" width="22" y="6" />
								<rect fill="#FFFFFF" height="16" width="4" x="7" />
								<rect fill="#00205B" height="2" width="22" y="7" />
								<rect fill="#00205B" height="16" width="2" x="8" />
							</svg>
							<span>Made in Norway.</span>
							<span className="text-verevon-j-text/34">Private by design.</span>
						</div>
					</div>

					<div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(170px,1.55fr)_minmax(0,0.9fr)] gap-x-[clamp(16px,2vw,32px)] gap-y-8 max-[900px]:grid-cols-2 max-[560px]:grid-cols-1">
						{footerColumns.map((column, columnIndex) => (
							<nav
								aria-label={`Bunntekstnavigasjon ${columnIndex + 1}`}
								className="grid content-start"
								key={columnIndex}
							>
								{column.map((link) => (
									<Link
										className="border-t border-verevon-j-text/15 py-[17px] text-[clamp(0.84rem,0.88vw,0.94rem)] font-normal uppercase leading-[1.25] tracking-[0.035em] text-verevon-j-text/85 transition-colors hover:text-[var(--verevon-coral-deep)]"
										href={link.href}
										key={link.label}
									>
										{link.label}
									</Link>
								))}
							</nav>
						))}

						<div className="col-start-1 mt-[clamp(24px,2.5vw,40px)] max-[900px]:col-start-auto max-[900px]:mt-8">
							<h4 className="mb-6 text-[clamp(0.8rem,0.84vw,0.9rem)] font-normal uppercase tracking-[0.035em] text-verevon-j-text/82">
								Hovedkontor
							</h4>

							<p className="m-0 text-[clamp(0.9rem,0.95vw,1rem)] font-normal leading-[1.5] text-verevon-j-text/74">
								Oslo, Norge
								<br />
								Distribuert team
								<br />
								Standardmodeller satt til Sweden Central (EU/EØS)
							</p>
						</div>

						<div className="col-start-2 mt-[clamp(24px,2.5vw,40px)] max-[900px]:col-start-auto max-[900px]:mt-8">
							<h4 className="mb-6 text-[clamp(0.8rem,0.84vw,0.9rem)] font-normal uppercase tracking-[0.035em] text-verevon-j-text/82">
								Kontakt oss
							</h4>

							<p className="m-0 text-[clamp(0.9rem,0.95vw,1rem)] font-normal leading-[1.5] text-verevon-j-text/74">
								<a
									className="transition-colors hover:text-[var(--verevon-coral-deep)]"
									href="mailto:hei@verevon.ai"
								>
									hei@verevon.ai
								</a>
								<br />
								<a
									className="transition-colors hover:text-[var(--verevon-coral-deep)]"
									href="/trust"
								>
									Kontakt sikkerhet
								</a>
							</p>
						</div>

						<div className="col-start-1 self-end max-[900px]:col-start-auto">
							<h4 className="text-[clamp(0.8rem,0.84vw,0.9rem)] font-normal uppercase tracking-[0.035em] text-verevon-j-text/82">
								Kontakt
							</h4>
						</div>

						<Link
							className="col-start-4 self-end justify-self-start text-[clamp(0.8rem,0.84vw,0.9rem)] font-normal uppercase tracking-[0.035em] text-verevon-j-text/82 transition-colors hover:text-[var(--verevon-coral-deep)] max-[900px]:col-start-auto"
							href="/#top"
						>
							<span aria-hidden="true">↑</span> Til toppen
						</Link>
					</div>
				</div>

				<div className="grid grid-cols-[minmax(180px,0.9fr)_minmax(0,3.1fr)] gap-[clamp(48px,6vw,112px)] border-t border-verevon-j-text/10 py-4 max-[900px]:grid-cols-1">
					<div />

					<div className="grid grid-cols-3 items-center gap-6 text-[clamp(0.72rem,0.78vw,0.84rem)] font-normal text-verevon-j-text/64 max-[900px]:grid-cols-1">
						<div>
							{contactLinks.map((link, index) => (
								<span key={link.href}>
									<a
										className="transition-colors hover:text-[var(--verevon-coral-deep)]"
										href={link.href}
									>
										{link.label}
									</a>
									{index < contactLinks.length - 1
										? " / "
										: ""}
								</span>
							))}
						</div>

						<a
							className="justify-self-center transition-colors hover:text-[var(--verevon-coral-deep)] max-[900px]:justify-self-start"
							href="/trust"
						>
							Sikkerhet og tillit
						</a>

						<small className="justify-self-end text-[clamp(0.72rem,0.78vw,0.84rem)] font-normal max-[900px]:justify-self-start">
							© 2024–{currentYear} Verevon AS
						</small>
					</div>
				</div>
			</div>
		</footer>
	);
}
