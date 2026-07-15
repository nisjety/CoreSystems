import Link from "next/link";

const pageLinks = [
	{ href: "#arbeidsflyt", label: "Arbeidsflyt" },
	{ href: "#bevis", label: "Grunnlag" },
	{ href: "#manuelt", label: "Gjør det selv" },
];

/** Compact, route-specific navigation for the workflow product story. */
export function WorkflowHeader() {
	return (
		<header className="sticky top-0 z-50 border-b border-velion-j-text/8 bg-[color-mix(in_srgb,var(--background)_92%,transparent)] backdrop-blur-[14px]">
			<div className="mx-auto flex h-[72px] max-w-[1680px] items-center justify-between px-[var(--velion-page-pad)]">
				<Link
					aria-label="Velion forside"
					className="font-arbeit text-base font-normal uppercase leading-none tracking-[0.44em] text-velion-j-text"
					href="/"
				>
					Velion
				</Link>

				<nav
					aria-label="Arbeidsflyt-navigasjon"
					className="flex items-center gap-[clamp(18px,3vw,42px)] font-protokoll text-[0.75rem] font-medium uppercase tracking-[0.06em] text-velion-j-text/60"
				>
					{pageLinks.map((link, index) => (
						<a
							className={[
								"transition-colors hover:text-velion-j-text",
								index > 0 ? "max-[620px]:hidden" : "",
							]
								.filter(Boolean)
								.join(" ")}
							href={link.href}
							key={link.href}
						>
							{link.label}
						</a>
					))}
					<Link className="transition-colors hover:text-velion-j-text" href="/">
						Forsiden
					</Link>
				</nav>
			</div>
		</header>
	);
}

export default WorkflowHeader;
