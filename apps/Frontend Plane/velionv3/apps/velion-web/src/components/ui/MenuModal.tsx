import type { CSSProperties } from "react";

type MenuModalProps = {
	onClose: () => void;
	open: boolean;
};

const menuItems = [
	{ href: "#product-loop", label: "Demo", number: "001" },
	{ href: "#workflows", label: "Workflows", number: "002" },
	{ href: "#trust", label: "Trust", number: "003" },
	{ href: "#contact", label: "Access", number: "004" },
];

const socialLinks = ["Instagram", "LinkedIn", "YouTube"];

export function MenuModal({ onClose, open }: MenuModalProps) {
	if (!open) {
		return null;
	}

	return (
		<div
			aria-label="Velion menu"
			aria-modal="true"
			className="fixed inset-0 z-[100] grid grid-rows-[auto_1fr_auto] bg-[linear-gradient(115deg,rgba(121,56,25,0.78),transparent_52%),linear-gradient(135deg,var(--velion-a-earth),var(--velion-h-ink-warm)_68%,#080707)] px-[clamp(24px,4vw,56px)] pb-14 pt-[54px] text-velion-c-white animate-[velion-menu-enter_420ms_ease_both] max-[760px]:pb-[34px] max-[760px]:pt-[34px]"
			role="dialog"
		>
			<div className="flex items-start justify-between">
				<a
					aria-label="Velion home"
					className="relative font-protokoll text-[3.4rem] font-medium leading-none before:absolute before:left-0 before:top-[0.56em] before:h-0.5 before:w-[1.1em] before:-translate-x-[0.38em] before:bg-current before:content-['']"
					href="#top"
					onClick={onClose}
				>
					V
				</a>

				<button
					aria-label="Close menu"
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
				aria-label="Primary menu"
				className="grid grid-cols-4 gap-7 self-center max-[1100px]:grid-cols-2 max-[760px]:grid-cols-1 max-[760px]:gap-[22px]"
			>
				{menuItems.map((item, index) => (
					<a
						className="grid translate-y-[26px] gap-[38px] border-t border-white/20 pt-2 opacity-0 animate-[velion-menu-item-enter_560ms_ease_forwards] [animation-delay:var(--delay)] transition-colors hover:text-white max-[760px]:gap-[18px]"
						href={item.href}
						key={item.href}
						onClick={onClose}
						style={{ "--delay": `${index * 80}ms` } as CSSProperties}
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
				aria-label="Social links"
				className="grid justify-items-start gap-2 font-arbeit text-base text-[color-mix(in_srgb,var(--velion-c-white)_74%,transparent)]"
			>
				<span>Follow us:</span>

				{socialLinks.map((link) => (
					<a
						className="transition-colors hover:text-velion-c-white"
						href="#contact"
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