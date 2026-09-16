import Link from "next/link";
import type { RefObject } from "react";

type NavbarProps = {
	isHidden?: boolean;
	isMenuOpen: boolean;
	isOnDark: boolean;
	isScrolled: boolean;
	menuButtonRef: RefObject<HTMLButtonElement | null>;
	onOpen: () => void;
};

const navItems = [
	{ href: "/produkt/arbeidsflyten", label: "Produkt" },
	{ href: "/plattform/felles-kontekst", label: "Plattform" },
	{ href: "/trust", label: "Tillit" },
	{ href: "/#kontakt", label: "Kontakt" },
];

export function Navbar({
	isHidden = false,
	isMenuOpen,
	isOnDark,
	isScrolled,
	menuButtonRef,
	onOpen,
}: NavbarProps) {
	const tone = isScrolled
		? "text-[color-mix(in_srgb,var(--verevon-j-text)_88%,transparent)]"
		: "text-verevon-c-white";
	const shouldUseHeroShadow = isOnDark || !isScrolled;

	return (
		<header
			className={[
				"pointer-events-none fixed inset-x-0 top-0 z-[60] flex items-center transition-[color,opacity] duration-[640ms] ease-[var(--verevon-ease-out)]",
				isHidden ? "opacity-0 [&_*]:pointer-events-none" : "",
				tone,
				shouldUseHeroShadow ? "verevon-hero-nav-shadow" : "",
			].join(" ")}
			data-scrolled={isScrolled ? "true" : "false"}
			data-site-nav=""
		>
			<div
				className="verevon-site-nav-surface pointer-events-none grid h-full w-full grid-cols-[minmax(160px,1fr)_auto_minmax(160px,1fr)] items-center border-b border-transparent px-[var(--verevon-edge)] transition-[background-color,border-color,box-shadow,opacity] duration-[640ms] ease-[var(--verevon-ease-out)] max-[1050px]:grid-cols-[1fr_auto] max-[1050px]:px-[var(--verevon-page-pad)]"
				data-scrolled={isScrolled ? "true" : "false"}
				data-site-nav-surface=""
			>
				<Link
					aria-label="Verevon hjem"
					className="pointer-events-auto inline-flex h-11 items-center justify-self-start font-arbeit text-[0.98rem] font-normal uppercase leading-none tracking-[0.18em] max-[1050px]:text-[0.86rem] max-[1050px]:tracking-[0.16em]"
					href="/"
				>
					VEREVON
				</Link>

				<nav
					aria-label="Hovednavigasjon"
					className="pointer-events-auto flex h-full items-center gap-[clamp(24px,3.4vw,64px)] justify-self-center max-[1050px]:hidden"
				>
					{navItems.map((item, index) => (
						<Link
							className={[
								"relative inline-flex h-full items-center justify-center font-protokoll text-[0.78rem] font-medium uppercase leading-none tracking-[0.06em] text-current opacity-75 transition-[color,opacity] after:absolute after:bottom-[11px] after:left-1/2 after:h-px after:w-[34px] after:-translate-x-1/2 after:scale-x-50 after:bg-current after:opacity-0 after:transition after:content-[''] hover:opacity-100 hover:after:scale-x-100 hover:after:opacity-80",
								index === 0
									? "opacity-100 after:scale-x-100 after:opacity-80"
									: "",
							].join(" ")}
							href={item.href}
							key={item.href}
						>
							{item.label}
						</Link>
					))}
				</nav>

				<div className="pointer-events-auto flex items-center gap-[18px] justify-self-end max-[1050px]:gap-[14px]">
					<a
						aria-label="Åpne Verevon dashboard"
						className="inline-flex min-h-11 items-center gap-2 rounded-full border border-current/20 px-4 font-protokoll text-[0.78rem] text-current transition-colors hover:bg-white/15 focus-visible:outline-2 focus-visible:outline-offset-4"
						href="http://localhost:5173"
					>
						Dashboard <span aria-hidden="true">↗</span>
					</a>

					<button
						aria-expanded={isMenuOpen}
						aria-label="Åpne meny"
						className="group grid h-11 w-[50px] cursor-pointer content-center gap-1.5 border-0 bg-transparent text-current opacity-70 transition-opacity hover:opacity-100 max-[1050px]:w-11"
						onClick={onOpen}
						ref={menuButtonRef}
						type="button"
					>
						<span className="block h-px w-full origin-right bg-current transition-transform group-hover:scale-x-[0.76]" />
						<span className="block h-px w-full origin-right bg-current transition-transform group-hover:scale-x-90" />
					</button>
				</div>
			</div>
		</header>
	);
}
