import Link from "next/link";

/** Minimal sticky header for the standalone Trust Center route. */
export function TrustHeader() {
	return (
		<header className="sticky top-0 z-50 border-b border-velion-j-text/8 bg-[color-mix(in_srgb,var(--background)_86%,transparent)] backdrop-blur-[14px]">
			<div className="mx-auto flex h-[72px] max-w-[1680px] items-center justify-between px-[var(--velion-page-pad)]">
				<Link
					aria-label="Velion forside"
					className="font-arbeit text-base font-normal uppercase leading-none tracking-[0.44em] text-velion-j-text"
					href="/"
				>
					Velion
				</Link>

				<nav
					aria-label="Trust Center-navigasjon"
					className="flex items-center gap-[clamp(20px,3vw,42px)] font-protokoll text-[0.78rem] font-medium uppercase tracking-[0.06em] text-velion-j-text/60"
				>
					<a className="transition-colors hover:text-velion-j-text" href="#kontroller">
						Kontroller
					</a>
					<a
						className="transition-colors hover:text-velion-j-text max-[560px]:hidden"
						href="#sertifiseringer"
					>
						Sertifiseringer
					</a>
					<a
						className="transition-colors hover:text-velion-j-text max-[760px]:hidden"
						href="#underleverandorer"
					>
						Underleverandører
					</a>
					<Link className="transition-colors hover:text-velion-j-text" href="/">
						Forsiden
					</Link>
				</nav>
			</div>
		</header>
	);
}

export default TrustHeader;
