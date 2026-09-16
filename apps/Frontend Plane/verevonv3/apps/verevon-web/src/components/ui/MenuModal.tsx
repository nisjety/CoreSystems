import { useEffect, useRef, type CSSProperties, type KeyboardEvent } from "react";
import Link from "next/link";
import { VerevonMarkFilled } from "@/components/home/sections/VerevonMark";

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

const contactLinks = [
	{ href: "mailto:hei@verevon.ai", label: "hei@verevon.ai" },
	{ href: "/trust", label: "Sikkerhet og tillit" },
];

export function MenuModal({ onClose, open }: MenuModalProps) {
	const dialogRef = useRef<HTMLDivElement>(null);
	const closeButtonRef = useRef<HTMLButtonElement>(null);

	useEffect(() => {
		if (!open) {
			return;
		}

		const focusInitialControl = window.requestAnimationFrame(() => {
			closeButtonRef.current?.focus();
		});

		return () => window.cancelAnimationFrame(focusInitialControl);
	}, [open]);

	const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
		if (event.key === "Escape") {
			event.preventDefault();
			onClose();
			return;
		}

		if (event.key !== "Tab") {
			return;
		}

		const dialog = dialogRef.current;
		if (!dialog) {
			return;
		}

		const focusableElements = Array.from(
			dialog.querySelectorAll<HTMLElement>(
				'a[href], button:not([disabled]), [tabindex]:not([tabindex="-1"])',
			),
		).filter((element) => !element.hasAttribute("disabled"));

		if (focusableElements.length === 0) {
			event.preventDefault();
			dialog.focus();
			return;
		}

		const firstElement = focusableElements[0];
		const lastElement = focusableElements[focusableElements.length - 1];
		const activeElement = document.activeElement;

		if (event.shiftKey && activeElement === firstElement) {
			event.preventDefault();
			lastElement.focus();
		} else if (!event.shiftKey && activeElement === lastElement) {
			event.preventDefault();
			firstElement.focus();
		}
	};

	if (!open) {
		return null;
	}

	return (
		<div
			aria-label="Verevon meny"
			aria-modal="true"
			className="fixed inset-0 z-[100] flex min-h-0 flex-col overflow-y-auto verevon-menu-surface px-[clamp(24px,4vw,56px)] pb-14 pt-[54px] text-verevon-j-text animate-[verevon-menu-enter_420ms_ease_both] max-[760px]:pb-[34px] max-[760px]:pt-[34px]"
			onKeyDown={handleKeyDown}
			ref={dialogRef}
			role="dialog"
			tabIndex={-1}
		>
			<div className="flex items-start justify-between">
				<a
					aria-label="Verevon hjem"
					className="grid size-[76px] place-items-start text-current transition-opacity hover:opacity-75 max-[760px]:size-[60px]"
					href="#top"
					onClick={onClose}
				>
					<VerevonMarkFilled className="size-full scale-170" />
				</a>

				<button
					aria-label="Lukk meny"
					className="grid h-[58px] w-[58px] cursor-pointer place-items-center border-0 bg-transparent p-0 text-current transition-opacity hover:opacity-70"
					onClick={onClose}
					ref={closeButtonRef}
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
				className="my-auto grid grid-cols-4 gap-7 py-[clamp(36px,9vh,144px)] max-[1100px]:grid-cols-2 max-[760px]:grid-cols-1 max-[760px]:gap-[22px] max-[760px]:py-8"
			>
				{menuItems.map((item, index) => {
					const itemContent = (
						<>
							<span className="font-arbeit text-[0.98rem] text-[color-mix(in_srgb,var(--verevon-j-text)_72%,transparent)]">
								{item.number}
							</span>

							<strong className="font-arbeit text-[2.2rem] font-normal leading-none text-[color-mix(in_srgb,var(--verevon-j-text)_78%,transparent)] max-[760px]:text-[1.72rem]">
								{item.label}
							</strong>
						</>
					);
					const itemProps = {
						className:
							"grid translate-y-[26px] gap-[38px] border-t border-black/15 pt-2 opacity-0 animate-[verevon-menu-item-enter_560ms_ease_forwards] [animation-delay:var(--delay)] transition-colors hover:text-black max-[760px]:gap-[18px]",
						href: item.href,
						onClick: onClose,
						style: { "--delay": `${index * 80}ms` } as CSSProperties,
					};

					return item.href.startsWith("/") ? (
						<Link {...itemProps} key={item.href}>
							{itemContent}
						</Link>
					) : (
						<a {...itemProps} key={item.href}>
							{itemContent}
						</a>
					);
				})}
			</nav>

			<div
				aria-label="Kontaktlenker"
				className="grid shrink-0 justify-items-start gap-2 font-arbeit text-base text-[color-mix(in_srgb,var(--verevon-j-text)_74%,transparent)]"
			>
				<span>Kontakt:</span>

				{contactLinks.map((link) => (
					<a
						className="transition-colors hover:text-verevon-j-text"
						href={link.href}
						key={link.href}
						onClick={onClose}
					>
						{link.label}
					</a>
				))}
			</div>
		</div>
	);
}
