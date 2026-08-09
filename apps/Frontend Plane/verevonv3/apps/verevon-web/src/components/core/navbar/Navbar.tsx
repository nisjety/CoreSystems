import Link from "next/link";

type NavbarProps = {
	isHidden?: boolean;
	isMenuOpen: boolean;
	isOnDark: boolean;
	isScrolled: boolean;
	onOpen: () => void;
};

const navItems = [
	{ href: "/produkt/arbeidsflyten", label: "Produkt" },
	{ href: "/plattform/felles-kontekst", label: "Plattform" },
	{ href: "/trust", label: "Tillit" },
	{ href: "#kontakt", label: "Kontakt" },
];

export function Navbar({
	isHidden = false,
	isMenuOpen,
	isOnDark,
	isScrolled,
	onOpen,
}: NavbarProps) {
	const tone =
		isOnDark || !isScrolled
			? "text-verevon-c-white"
			: "text-[color-mix(in_srgb,var(--verevon-text)_76%,transparent)]";
	const shouldUseHeroShadow = isOnDark || !isScrolled;

	return (
		<header
			className={[
				"pointer-events-none fixed inset-x-0 top-0 z-[60] grid h-28 grid-cols-[minmax(160px,1fr)_auto_minmax(160px,1fr)] items-center px-[clamp(24px,4vw,56px)] transition-[color,height,opacity] duration-300 max-[760px]:h-24 max-[760px]:grid-cols-[1fr_auto]",
				isHidden ? "opacity-0 [&_*]:pointer-events-none" : "",
				isScrolled ? "h-[92px]" : "",
				isOnDark
					? "text-[color-mix(in_srgb,var(--verevon-c-white)_76%,transparent)]"
					: tone,
				shouldUseHeroShadow ? "verevon-hero-nav-shadow" : "",
			].join(" ")}
			data-site-nav=""
		>
			<a
				aria-label="Verevon hjem"
				className="pointer-events-auto justify-self-start font-arbeit text-[0.98rem] font-normal uppercase leading-none tracking-[0.18em] max-[760px]:text-[0.86rem] max-[760px]:tracking-[0.16em]"
				href="#top"
			>
				VEREVON
			</a>

			<nav
				aria-label="Hovednavigasjon"
				className="pointer-events-auto flex items-center gap-[clamp(42px,5.25vw,90px)] justify-self-center max-[760px]:hidden"
			>
				{navItems.map((item, index) => (
					<Link
						className={[
							"relative inline-flex items-center justify-center py-[41px] pb-7 font-protokoll text-[0.78rem] font-medium uppercase leading-none tracking-[0.06em] text-[color-mix(in_srgb,currentColor_76%,transparent)] transition-colors after:absolute after:bottom-[17px] after:left-1/2 after:h-px after:w-[34px] after:-translate-x-1/2 after:scale-x-50 after:bg-current after:opacity-0 after:transition after:content-[''] hover:text-current hover:after:scale-x-100 hover:after:opacity-80",
							index === 0
								? "text-current after:scale-x-100 after:opacity-80"
								: "",
						].join(" ")}
						href={item.href}
						key={item.href}
					>
						{item.label}
					</Link>
				))}
			</nav>

			<div className="pointer-events-auto flex items-center gap-[30px] justify-self-end max-[760px]:gap-[22px]">
				<a
					aria-label="Søk"
					className="grid size-[34px] place-items-center text-[color-mix(in_srgb,currentColor_58%,transparent)] transition-colors hover:text-current max-[760px]:size-8"
					href="#kontakt"
				>
					<svg
						aria-hidden="true"
						className="size-[18px] overflow-visible fill-none stroke-current stroke-[1px] max-[760px]:size-[17px]"
						viewBox="0 0 20 20"
					>
						<circle cx="8.5" cy="8.5" r="5.75" />
						<path d="m13 13 4 4" strokeLinecap="round" />
					</svg>
				</a>

				<button
					aria-expanded={isMenuOpen}
					aria-label="Åpne meny"
					className="group grid w-[50px] cursor-pointer gap-1.5 border-0 bg-transparent py-2.5 text-[color-mix(in_srgb,currentColor_68%,transparent)] transition-colors hover:text-current max-[760px]:w-11"
					onClick={onOpen}
					type="button"
				>
					<span className="block h-px w-full origin-right bg-current transition-transform group-hover:scale-x-[0.76]" />
					<span className="block h-px w-full origin-right bg-current transition-transform group-hover:scale-x-90" />
				</button>
			</div>
		</header>
	);
}
