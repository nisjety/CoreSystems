import type {
	AnchorHTMLAttributes,
	ButtonHTMLAttributes,
	ReactNode,
} from "react";

type ArrowButtonBaseProps = {
	children: ReactNode;
	className?: string;
	variant?: "dark" | "light" | "muted" | "coral";
};

type ArrowLinkButtonProps = ArrowButtonBaseProps &
	Omit<AnchorHTMLAttributes<HTMLAnchorElement>, "children" | "className"> & {
		href: string;
	};

type ArrowActionButtonProps = ArrowButtonBaseProps &
	Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children" | "className"> & {
		href?: undefined;
	};

type ArrowButtonProps = ArrowLinkButtonProps | ArrowActionButtonProps;

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

export function ArrowButtonLabel({
	children,
	className = "",
}: {
	children: ReactNode;
	className?: string;
}) {
	return (
		<span
			className={[
				"relative inline-flex w-fit items-center overflow-hidden px-px py-1 font-protokoll text-[clamp(0.94rem,0.95vw,1.08rem)] font-light leading-none opacity-80 transition-all duration-500 group-hover:opacity-100",
				className,
			]
				.filter(Boolean)
				.join(" ")}
		>
			<span className="flex translate-x-[-43px] items-center gap-3 transition-transform duration-500 group-hover:translate-x-0 sm:translate-x-[-37px] sm:gap-1">
				<ArrowIcon />

				<span className="whitespace-nowrap">{children}</span>

				<ArrowIcon className="absolute left-full translate-x-3 sm:translate-x-1" />
			</span>
		</span>
	);
}

export function ArrowButton(props: ArrowButtonProps) {
	const {
		children,
		className = "",
		variant = "dark",
	} = props;

	const wrapperClasses = [
		"group inline-flex w-fit items-center bg-transparent p-0 text-left no-underline",
		className,
	]
		.filter(Boolean)
		.join(" ");

	const content = (
		<ArrowButtonLabel className={variantClasses[variant]}>
			{children}
		</ArrowButtonLabel>
	);

	if ("href" in props && props.href) {
		const {
			children: ignoredChildren,
			className: ignoredClassName,
			href,
			variant: ignoredVariant,
			...linkProps
		} = props as ArrowLinkButtonProps;

		void ignoredChildren;
		void ignoredClassName;
		void ignoredVariant;

		return (
			<a {...linkProps} className={wrapperClasses} href={href}>
				{content}
			</a>
		);
	}

	const {
		children: ignoredChildren,
		className: ignoredClassName,
		type = "button",
		variant: ignoredVariant,
		...buttonProps
	} = props as ArrowActionButtonProps;

	void ignoredChildren;
	void ignoredClassName;
	void ignoredVariant;

	return (
		<button {...buttonProps} className={wrapperClasses} type={type}>
			{content}
		</button>
	);
}
