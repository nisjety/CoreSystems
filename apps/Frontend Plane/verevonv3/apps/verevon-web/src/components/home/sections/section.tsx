import { forwardRef, type ReactNode } from "react";

type SectionProps = {
	children: ReactNode;
	containerClassName?: string;
	id: string;
	subtitle?: ReactNode;
	subtitleClassName?: string;
	title: ReactNode;
	titleClassName?: string;
	variant?: "default" | "full-bleed" | "full-bleed-tight" | "medium-bleed";
};

export const Section = forwardRef<HTMLElement, SectionProps>(
	(
		{
			children,
			containerClassName,
			id,
			subtitle,
			subtitleClassName,
			title,
			titleClassName,
			variant = "default",
		},
		ref,
	) => {
		const getVariantClasses = () => {
			switch (variant) {
				case "full-bleed":
					return [
						"relative left-1/2 right-1/2 w-screen",
						"-ml-[50vw] -mr-[50vw]",
						"px-4 py-20",
						containerClassName,
					]
						.filter(Boolean)
						.join(" ");
				case "full-bleed-tight":
					return [
						"relative left-1/2 right-1/2 w-screen",
						"-ml-[50vw] -mr-[50vw]",
						"p-0",
						containerClassName,
					]
						.filter(Boolean)
						.join(" ");
				case "medium-bleed":
					return [
						"relative left-1/2 right-1/2 w-screen",
						"-ml-[50vw] -mr-[50vw]",
						"px-[38px] py-4",
						containerClassName,
					]
						.filter(Boolean)
						.join(" ");
				default:
					return ["mx-auto max-w-6xl px-6 py-16", containerClassName]
						.filter(Boolean)
						.join(" ");
			}
		};

		return (
			<section className={getVariantClasses()} id={id} ref={ref}>
				<header className="mb-4 space-y-1">
					<h2
						className={
							titleClassName ?? "text-2xl font-semibold tracking-tight"
						}
					>
						{title}
					</h2>

					{subtitle ? (
						<p
							className={
								subtitleClassName ??
								"max-w-3xl text-sm text-[rgb(var(--muted))]"
							}
						>
							{subtitle}
						</p>
					) : null}
				</header>

				{children}
			</section>
		);
	},
);

Section.displayName = "Section";
