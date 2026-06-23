const footerColumns = [
	[
		{ href: "#product-loop", label: "Product" },
		{ href: "#workflows", label: "Workflows" },
		{ href: "#trust", label: "Trust" },
	],
	[
		{ href: "#technology", label: "System" },
		{ href: "#company", label: "Company" },
		{ href: "#reports", label: "Reports" },
	],
	[
		{ href: "#updates", label: "News" },
		{ href: "#demo", label: "Velion Console ↗" },
		{ href: "#careers", label: "Careers" },
	],
	[
		{ href: "#contact", label: "Contact" },
		{ href: "#security", label: "Security ↗" },
		{ href: "#access", label: "Early Access" },
	],
];

const socialLinks = ["LinkedIn", "Instagram", "YouTube"];

export function Footer() {
	const currentYear = new Date().getFullYear();

	return (
		<footer
			className="relative overflow-hidden bg-velion-footer-bg text-velion-j-text"
			id="contact"
			data-footer-parallax=""
		>
			<div
				className="px-[clamp(24px,4vw,72px)] pb-0 pt-[clamp(44px,5vw,82px)]"
				data-footer-parallax-media=""
			>
				<div className="grid min-h-[520px] grid-cols-[minmax(180px,0.9fr)_minmax(0,3.1fr)] gap-[clamp(48px,6vw,112px)] max-[900px]:min-h-0 max-[900px]:grid-cols-1">
					<div className="relative top-10 grid content-start gap-8">
              <a
                aria-label="Velion home"
                className="font-arbeit text-[clamp(2rem,2.4vw,3rem)] font-light leading-none tracking-[-0.055em] text-velion-j-text"
                href="#top"
              >
                VELION
              </a>

              <p className="m-0 max-w-[280px] font-protokoll text-[clamp(0.86rem,0.9vw,1rem)] font-light uppercase leading-[1.35] tracking-[0.02em] text-velion-j-text/45">
                VELION IS BUILT FOR COMPANIES THAT WANT AI TO DO REAL CUSTOMER WORK WITH
                SOURCES, APPROVALS, AUDIT HISTORY, AND HUMANS STILL IN CONTROL.
              </p>
            </div>

					<div className="grid grid-cols-4 gap-x-[clamp(24px,3vw,56px)] gap-y-20 max-[900px]:grid-cols-2 max-[560px]:grid-cols-1">
						{footerColumns.map((column, columnIndex) => (
							<nav
								aria-label={`Footer navigation ${columnIndex + 1}`}
								className="grid content-start"
								key={columnIndex}
							>
								{column.map((link) => (
									<a
										className="border-t border-velion-j-text/8 py-[18px] font-arbeit text-[clamp(0.88rem,0.9vw,1.05rem)] font-normal uppercase leading-none tracking-[0.08em] text-velion-j-text/65 transition-colors hover:text-velion-j-text"
										href={link.href}
										key={link.label}
									>
										{link.label}
									</a>
								))}
							</nav>
						))}

						<div className="col-start-1 mt-[clamp(42px,7vw,128px)] max-[900px]:col-start-auto max-[900px]:mt-8">
							<h4 className="mb-8 font-arbeit text-[clamp(0.84rem,0.86vw,1rem)] font-normal uppercase tracking-[0.08em] text-velion-j-text/70">
								Headquarters
							</h4>

							<p className="m-0 font-protokoll text-[clamp(1rem,1vw,1.18rem)] font-light leading-[1.35] text-velion-j-text/35">
								Oslo, Norway
								<br />
								Remote-first team
								<br />
								AI operations platform
							</p>
						</div>

						<div className="col-start-2 mt-[clamp(42px,7vw,128px)] max-[900px]:col-start-auto max-[900px]:mt-8">
							<h4 className="mb-8 font-arbeit text-[clamp(0.84rem,0.86vw,1rem)] font-normal uppercase tracking-[0.08em] text-velion-j-text/70">
								Contact us
							</h4>

							<p className="m-0 font-protokoll text-[clamp(1rem,1vw,1.18rem)] font-light leading-[1.35] text-velion-j-text/35">
								<a
									className="transition-colors hover:text-velion-j-text"
									href="mailto:hello@velion.ai"
								>
									hello@velion.ai
								</a>
								<br />
								<a
									className="transition-colors hover:text-velion-j-text"
									href="#access"
								>
									Request early access
								</a>
							</p>
						</div>

						<div className="col-start-1 self-end max-[900px]:col-start-auto">
							<h4 className="font-arbeit text-[clamp(0.84rem,0.86vw,1rem)] font-normal uppercase tracking-[0.08em] text-velion-j-text/70">
								Follow us
							</h4>
						</div>

						<a
							className="col-start-4 self-end justify-self-start font-protokoll text-[clamp(0.94rem,0.95vw,1.08rem)] font-light text-velion-j-text/80 transition-colors hover:text-velion-j-text max-[900px]:col-start-auto"
							href="#top"
						>
							<span aria-hidden="true">↑</span> To top
						</a>
					</div>
				</div>

				<div className="grid grid-cols-[minmax(180px,0.9fr)_minmax(0,3.1fr)] gap-[clamp(48px,6vw,112px)] border-t border-velion-j-text/8 py-8 max-[900px]:grid-cols-1">
					<div />

					<div className="grid grid-cols-3 items-center gap-6 font-protokoll text-[clamp(0.88rem,0.9vw,1rem)] font-light text-velion-j-text/50 max-[900px]:grid-cols-1">
						<div>
							{socialLinks.map((link, index) => (
								<span key={link}>
									<a
										className="transition-colors hover:text-velion-j-text"
										href="#contact"
									>
										{link}
									</a>
									{index < socialLinks.length - 1 ? " / " : ""}
								</span>
							))}
						</div>

						<a
							className="justify-self-center transition-colors hover:text-velion-j-text max-[900px]:justify-self-start"
							href="#top"
						>
							Design & Concept by Velion
						</a>

						<small className="justify-self-end font-protokoll text-[clamp(0.88rem,0.9vw,1rem)] font-light max-[900px]:justify-self-start">
							© 2024-{currentYear} Velion AS
						</small>
					</div>
				</div>
			</div>
		</footer>
	);
}