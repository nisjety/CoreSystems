import Link from "next/link";
import { usePathname } from "next/navigation";
import type { RefObject } from "react";

type NavbarProps = {
	isHidden?: boolean;
	isIntroActive?: boolean;
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
	isIntroActive = false,
	isMenuOpen,
	isScrolled,
	menuButtonRef,
	onOpen,
}: NavbarProps) {
	const pathname = usePathname();
	const isHeroSurface = !isScrolled;
	const tone = isHeroSurface ? "text-verevon-c-white" : "text-verevon-j-text";
	const shouldUseHeroShadow = isHeroSurface;

	return (
		<header
			className={[
				"pointer-events-none fixed inset-x-0 top-0 z-[60] flex items-center transition-opacity duration-[640ms] ease-[var(--verevon-ease-out)]",
				isHidden ? "opacity-0 [&_*]:pointer-events-none" : "",
				tone,
				shouldUseHeroShadow ? "verevon-hero-nav-shadow" : "",
			].join(" ")}
			data-scrolled={isScrolled ? "true" : "false"}
			data-site-nav=""
		>
			<div
				className="verevon-site-nav-surface pointer-events-none grid h-full w-full grid-cols-[minmax(160px,1fr)_auto_minmax(160px,1fr)] items-center border-b border-transparent px-[var(--verevon-edge)] max-[1050px]:grid-cols-[1fr_auto] max-[1050px]:px-[var(--verevon-page-pad)]"
				data-scrolled={isScrolled ? "true" : "false"}
				data-site-nav-surface=""
			>
				<Link
					aria-label="Verevon hjem"
					className="verevon-site-brand verevon-wordmark pointer-events-auto inline-flex h-8 items-center justify-self-start text-[0.9rem] font-normal uppercase leading-none tracking-[0.08em] max-[1050px]:text-[0.8rem] max-[1050px]:tracking-[0.08em]"
					data-intro-hidden={isIntroActive || undefined}
					data-site-brand-target=""
					href="/"
				>
					<span>VEREVON</span>
				</Link>

				<nav
					aria-label="Hovednavigasjon"
					className="pointer-events-auto flex h-full items-center gap-[clamp(24px,3.4vw,64px)] justify-self-center max-[1050px]:hidden"
					data-navigation-style="stingray"
				>
					{navItems.map((item) => {
						const isCurrent = pathname === item.href;

						return (
						<Link
							aria-current={isCurrent ? "page" : undefined}
							className={[
								"relative inline-flex h-full items-center justify-center font-geist text-[0.66rem] font-normal uppercase leading-none tracking-[0.045em] text-current transition-[color,opacity] after:absolute after:bottom-[7px] after:left-1/2 after:h-px after:w-[24px] after:-translate-x-1/2 after:scale-x-50 after:bg-current after:opacity-0 after:transition after:content-[''] hover:text-[var(--verevon-coral-deep)] hover:opacity-100 hover:after:scale-x-100 hover:after:opacity-75 focus-visible:text-[var(--verevon-coral-deep)] focus-visible:opacity-100 focus-visible:outline-2 focus-visible:outline-offset-4",
								isHeroSurface ? "opacity-90" : "opacity-70",
								isCurrent ? "opacity-100 after:scale-x-100 after:opacity-75" : "",
							].join(" ")}
							data-navigation-link="stingray"
							href={item.href}
							key={item.href}
						>
							{item.label}
						</Link>
						);
					})}
				</nav>

				<div className="pointer-events-auto flex items-center gap-[18px] justify-self-end max-[1050px]:gap-[14px]">
					<a
						aria-label="Åpne Verevon dashboard"
					className="verevon-dashboard-link inline-flex min-h-8 items-center gap-1.5 rounded-md border border-verevon-c-white/70 px-3 font-geist text-[0.7rem] font-medium text-verevon-j-text no-underline focus-visible:outline-2 focus-visible:outline-offset-4"
						data-dashboard-link=""
						href="http://localhost:5173"
					onClick={(event) => {
						if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
						event.preventDefault();
						const link = event.currentTarget;
						link.dataset.loading = "true";
						window.setTimeout(() => window.location.assign(link.href), 340);
					}}
					>
						Dashboard <span aria-hidden="true">↗</span>
					</a>

					<button
						aria-expanded={isMenuOpen}
						aria-label="Åpne meny"
						className={[
							"group grid h-8 w-[44px] cursor-pointer content-center gap-1.5 border-0 bg-transparent text-current transition-[color,opacity] hover:text-[var(--verevon-coral-deep)] hover:opacity-100 max-[1050px]:w-10",
							isHeroSurface ? "opacity-90" : "opacity-70",
						].join(" ")}
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
