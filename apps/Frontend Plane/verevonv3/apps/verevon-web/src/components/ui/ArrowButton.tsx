import type {
	AnchorHTMLAttributes,
	ButtonHTMLAttributes,
	ReactNode,
} from "react";
import Link from "next/link";

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
	dark: "text-verevon-coral",
	light: "text-verevon-c-white",
	muted: "text-verevon-coral/72",
	coral: "text-[var(--verevon-coral-deep)]",
};

function ArrowIcon({ className = "" }: { className?: string }) {
	return (
		<svg
			aria-hidden="true"
			className={["verevon-cta-arrow", className]
				.filter(Boolean)
				.join(" ")}
			fill="none"
			viewBox="0 0 48 8"
			xmlns="http://www.w3.org/2000/svg"
		>
			<path d="M0.5 4H46M42.5 0.5 46 4l-3.5 3.5" stroke="currentColor" strokeLinecap="square" />
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
				"verevon-cta-label",
				className,
			]
				.filter(Boolean)
				.join(" ")}
		>
			<span>{children}</span>
			<ArrowIcon />
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
		"verevon-cta group",
		variantClasses[variant],
		className,
	]
		.filter(Boolean)
		.join(" ");

	const content = <ArrowButtonLabel>{children}</ArrowButtonLabel>;

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

		if (href.startsWith("/") && !href.startsWith("//")) {
			return (
				<Link {...linkProps} className={wrapperClasses} data-cta-variant={variant} href={href}>
					{content}
				</Link>
			);
		}

		return (
			<a {...linkProps} className={wrapperClasses} data-cta-variant={variant} href={href}>
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
		<button {...buttonProps} className={wrapperClasses} data-cta-variant={variant} type={type}>
			{content}
		</button>
	);
}
