import Link from "next/link";

// Section links use absolute "/#anchor" form so they resolve from any route
// (e.g. the /trust page), not just the homepage.
const footerColumns = [
	[
		{ href: "/#produkt", label: "Produkt" },
		{ href: "/#plattform", label: "Plattform" },
		{ href: "/#kunnskap", label: "Kunnskap" },
	],
	[
		{ href: "/#flyt", label: "Arbeidssløyfen" },
	],
	[
		{ href: "/trust", label: "Tillitssenter ↗" },
		{ href: "/trust", label: "Datasuverenitet ↗" },
		{ href: "/trust", label: "Sikkerhet ↗" },
	],
	[
		{ href: "/#kontakt", label: "Kontakt" },
		{ href: "/#kontakt", label: "Tidlig tilgang" },
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
			className="relative overflow-hidden bg-verevon-footer-bg text-verevon-j-text lg:h-[73svh] lg:min-h-[73svh]"
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
							className="font-arbeit text-[clamp(1.8rem,2.16vw,2.7rem)] font-light leading-none tracking-[-0.055em] text-verevon-j-text"
							href="/#top"
						>
							VEREVON
						</Link>

						<p className="m-0 max-w-[320px] font-protokoll text-[clamp(0.9rem,0.95vw,1.08rem)] font-light uppercase leading-[1.32] tracking-[0.02em] text-verevon-j-text/65">
							Verevon hjelper teamet å svare raskere, handle
							tryggere og vise hvor AI-en fant grunnlaget.
						</p>
					</div>

					<div className="grid grid-cols-4 gap-x-[clamp(24px,3vw,56px)] gap-y-8 max-[900px]:grid-cols-2 max-[560px]:grid-cols-1">
						{footerColumns.map((column, columnIndex) => (
							<nav
								aria-label={`Bunntekstnavigasjon ${columnIndex + 1}`}
								className="grid content-start"
								key={columnIndex}
							>
								{column.map((link) => (
									<Link
										className="border-t border-verevon-j-text/8 py-[18px] font-arbeit text-[clamp(0.88rem,0.9vw,1.05rem)] font-normal uppercase leading-none tracking-[0.08em] text-verevon-j-text/65 transition-colors hover:text-verevon-j-text"
										href={link.href}
										key={link.label}
									>
										{link.label}
									</Link>
								))}
							</nav>
						))}

						<div className="col-start-1 mt-[clamp(24px,2.5vw,40px)] max-[900px]:col-start-auto max-[900px]:mt-8">
							<h4 className="mb-8 font-arbeit text-[clamp(0.84rem,0.86vw,1rem)] font-normal uppercase tracking-[0.08em] text-verevon-j-text/70">
								Hovedkontor
							</h4>

							<p className="m-0 font-protokoll text-[clamp(1rem,1vw,1.18rem)] font-light leading-[1.35] text-verevon-j-text/65">
								Oslo, Norge
								<br />
								Distribuert team
								<br />
								Data i EU/EØS (Sweden Central)
							</p>
						</div>

						<div className="col-start-2 mt-[clamp(24px,2.5vw,40px)] max-[900px]:col-start-auto max-[900px]:mt-8">
							<h4 className="mb-8 font-arbeit text-[clamp(0.84rem,0.86vw,1rem)] font-normal uppercase tracking-[0.08em] text-verevon-j-text/70">
								Kontakt oss
							</h4>

							<p className="m-0 font-protokoll text-[clamp(1rem,1vw,1.18rem)] font-light leading-[1.35] text-verevon-j-text/65">
								<a
									className="transition-colors hover:text-verevon-j-text"
									href="mailto:hei@verevon.ai"
								>
									hei@verevon.ai
								</a>
								<br />
								<a
									className="transition-colors hover:text-verevon-j-text"
									href="/trust"
								>
									Kontakt sikkerhet
								</a>
							</p>
						</div>

						<div className="col-start-1 self-end max-[900px]:col-start-auto">
							<h4 className="font-arbeit text-[clamp(0.84rem,0.86vw,1rem)] font-normal uppercase tracking-[0.08em] text-verevon-j-text/70">
								Kontakt
							</h4>
						</div>

						<Link
							className="col-start-4 self-end justify-self-start font-protokoll text-[clamp(0.94rem,0.95vw,1.08rem)] font-light text-verevon-j-text/80 transition-colors hover:text-verevon-j-text max-[900px]:col-start-auto"
							href="/#top"
						>
							<span aria-hidden="true">↑</span> Til toppen
						</Link>
					</div>
				</div>

				<div className="grid grid-cols-[minmax(180px,0.9fr)_minmax(0,3.1fr)] gap-[clamp(48px,6vw,112px)] border-t border-verevon-j-text/8 py-4 max-[900px]:grid-cols-1">
					<div />

					<div className="grid grid-cols-3 items-center gap-6 font-protokoll text-[clamp(0.88rem,0.9vw,1rem)] font-light text-verevon-j-text/65 max-[900px]:grid-cols-1">
						<div>
							{contactLinks.map((link, index) => (
								<span key={link.href}>
									<a
										className="transition-colors hover:text-verevon-j-text"
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
							className="justify-self-center transition-colors hover:text-verevon-j-text max-[900px]:justify-self-start"
							href="/trust"
						>
							Sikkerhet og tillit
						</a>

						<small className="justify-self-end font-protokoll text-[clamp(0.88rem,0.9vw,1rem)] font-light max-[900px]:justify-self-start">
							© 2024–{currentYear} Verevon AS
						</small>
					</div>
				</div>
			</div>
		</footer>
	);
}
