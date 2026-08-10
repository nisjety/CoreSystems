import Link from "next/link";

const pageLinks = [
	{ href: "#problemet", label: "Problemet" },
	{ href: "#losning", label: "Løsningen" },
];

/** A compact, route-specific header for the response-time product page. */
export function ResponseTimeHeader() {
	return (
		<header className="sticky top-0 z-50 border-b border-verevon-j-text/8 bg-[color-mix(in_srgb,var(--background)_92%,transparent)] backdrop-blur-[14px]">
			<div className="mx-auto flex h-[72px] max-w-[1680px] items-center justify-between px-[var(--verevon-page-pad)]">
				<Link
					aria-label="Verevon forside"
					className="font-arbeit text-base font-normal uppercase leading-none tracking-[0.44em] text-verevon-j-text"
					href="/"
				>
					Verevon
				</Link>

				<nav
					aria-label="Svartid-navigasjon"
					className="flex items-center gap-[clamp(18px,3vw,42px)] font-protokoll text-[0.75rem] font-medium uppercase tracking-[0.06em] text-verevon-j-text/60"
				>
					{pageLinks.map((link, index) => (
						<a
							className={[
								"transition-colors hover:text-verevon-j-text focus-visible:text-verevon-j-text focus-visible:outline-none",
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
					<Link
						className="transition-colors hover:text-verevon-j-text focus-visible:text-verevon-j-text focus-visible:outline-none"
						href="/"
					>
						Forsiden
					</Link>
				</nav>
			</div>
		</header>
	);
}

export default ResponseTimeHeader;
