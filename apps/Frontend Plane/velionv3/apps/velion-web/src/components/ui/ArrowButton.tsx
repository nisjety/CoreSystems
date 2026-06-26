import type { ReactNode } from "react";

type ArrowButtonProps = {
	children: ReactNode;
	className?: string;
	href?: string;
	onClick?: () => void;
	type?: "button" | "submit" | "reset";
	variant?: "dark" | "light" | "muted" | "coral";
};

const variantClasses = {
	dark: "text-velion-j-text",
	light: "text-velion-c-white",
	muted: "text-velion-j-text/60",
	coral: "text-velion-coral",
};

function ArrowIcon({ className = "" }: { className?: string }) {
	return (
		<svg
			aria-hidden="true"
			className={["h-[10px] fill-current lg:h-[11px]", className]
				.filter(Boolean)
				.join(" ")}
			viewBox="0 0 22.35 7.16"
			xmlns="http://www.w3.org/2000/svg"
		>
			<path
				d="m18.77 0 3.58 3.58c-.76 0-1.52-.29-2.1-.87l-2.1-2.1.62-.61zm-.61 6.54 2.1-2.1c.58-.58 1.34-.87 2.1-.87l-3.58 3.58-.62-.61zm.28-2.53v-.87H0V4h18.44z"
				strokeWidth="0.5px"
			/>
		</svg>
	);
}

export function ArrowButton({
	children,
	className = "",
	href,
	onClick,
	type = "button",
	variant = "dark",
}: ArrowButtonProps) {
	const classes = [
		"group relative inline-flex w-fit items-center overflow-hidden px-px py-1 font-protokoll text-[clamp(0.94rem,0.95vw,1.08rem)] font-light leading-none opacity-80 transition-all duration-500 hover:opacity-100",
		variantClasses[variant],
		className,
	]
		.filter(Boolean)
		.join(" ");

	const content = (
		<span className="flex translate-x-[-43px] items-center gap-3 transition-transform duration-500 group-hover:translate-x-0 sm:translate-x-[-37px] sm:gap-1">
			<ArrowIcon />

			<span className="whitespace-nowrap">{children}</span>

			<ArrowIcon className="absolute left-full translate-x-3 sm:translate-x-1" />
		</span>
	);

	if (href) {
		return (
			<a className={classes} href={href}>
				{content}
			</a>
		);
	}

	return (
		<button className={classes} onClick={onClick} type={type}>
			{content}
		</button>
	);
}