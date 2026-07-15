import type { CSSProperties } from "react";
import { VelionMarkFilled } from "@/components/home/sections/VelionMark";

type MenuModalProps = {
	onClose: () => void;
	open: boolean;
};

const menuItems = [
	{ href: "/produkt/arbeidsflyten", label: "Produkt", number: "001" },
	{ href: "/plattform/felles-kontekst", label: "Plattform", number: "002" },
	{ href: "/trust", label: "Tillit", number: "003" },
	{ href: "#kontakt", label: "Kontakt", number: "004" },
];

const socialLinks = ["Instagram", "LinkedIn", "YouTube"];

export function MenuModal({ onClose, open }: MenuModalProps) {
	if (!open) {
		return null;
	}

	return (
		<div
			aria-label="Velion meny"
			aria-modal="true"
			className="fixed inset-0 z-[100] grid grid-rows-[auto_1fr_auto] bg-[linear-gradient(115deg,rgba(121,56,25,0.78),transparent_52%),linear-gradient(135deg,var(--velion-a-earth),var(--velion-h-ink-warm)_68%,#080707)] px-[clamp(24px,4vw,56px)] pb-14 pt-[54px] text-velion-c-white animate-[velion-menu-enter_420ms_ease_both] max-[760px]:pb-[34px] max-[760px]:pt-[34px]"
			role="dialog"
		>
			<div className="flex items-start justify-between">
				<a
					aria-label="Velion hjem"
					className="grid size-[76px] place-items-start text-current transition-opacity hover:opacity-75 max-[760px]:size-[60px]"
					href="#top"
					onClick={onClose}
				>
					<VelionMarkFilled className="size-full scale-170" />
				</a>

				<button
					aria-label="Lukk meny"
					className="grid h-[58px] w-[58px] cursor-pointer place-items-center border-0 bg-transparent p-0 text-current transition-opacity hover:opacity-70"
					onClick={onClose}
					type="button"
				>
					<svg
						aria-hidden="true"
						className="size-full fill-none stroke-current stroke-[1.5]"
						viewBox="0 0 40 40"
					>
						<path d="M6 6l28 28M34 6 6 34" />
					</svg>
				</button>
			</div>

			<nav
				aria-label="Hovedmeny"
				className="grid grid-cols-4 gap-7 self-center max-[1100px]:grid-cols-2 max-[760px]:grid-cols-1 max-[760px]:gap-[22px]"
			>
				{menuItems.map((item, index) => (
					<a
						className="grid translate-y-[26px] gap-[38px] border-t border-white/20 pt-2 opacity-0 animate-[velion-menu-item-enter_560ms_ease_forwards] [animation-delay:var(--delay)] transition-colors hover:text-white max-[760px]:gap-[18px]"
						href={item.href}
						key={item.href}
						onClick={onClose}
						style={
							{ "--delay": `${index * 80}ms` } as CSSProperties
						}
					>
						<span className="font-arbeit text-[0.98rem] text-[color-mix(in_srgb,var(--velion-c-white)_72%,transparent)]">
							{item.number}
						</span>

						<strong className="font-arbeit text-[2.2rem] font-normal leading-none text-[color-mix(in_srgb,var(--velion-c-white)_78%,transparent)] max-[760px]:text-[1.72rem]">
							{item.label}
						</strong>
					</a>
				))}
			</nav>

			<div
				aria-label="Sosiale lenker"
				className="grid justify-items-start gap-2 font-arbeit text-base text-[color-mix(in_srgb,var(--velion-c-white)_74%,transparent)]"
			>
				<span>Følg oss:</span>

				{socialLinks.map((link) => (
					<a
						className="transition-colors hover:text-velion-c-white"
						href="#kontakt"
						key={link}
						onClick={onClose}
					>
						{link}
					</a>
				))}
			</div>
		</div>
	);
}
