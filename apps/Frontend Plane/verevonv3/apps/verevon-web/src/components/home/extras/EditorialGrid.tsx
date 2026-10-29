type EditorialGridProps = {
	className?: string;
	tone?: "dark" | "light";
};

export function EditorialGrid({
	className = "",
	tone = "light",
}: EditorialGridProps) {
	const colorClass =
		tone === "dark" ? "text-verevon-c-white/12" : "text-verevon-j-text/10";

	return (
		<div
			aria-hidden="true"
			className={[
				"pointer-events-none absolute inset-0 overflow-hidden will-change-transform",
				colorClass,
				className,
			]
				.filter(Boolean)
				.join(" ")}
			data-editorial-grid=""
		>
			<div className="absolute inset-y-0 left-[var(--verevon-edge)] right-[var(--verevon-edge)] max-[760px]:left-[var(--verevon-page-pad)] max-[760px]:right-[var(--verevon-page-pad)]">
				<span className="absolute inset-y-0 left-0 w-px bg-current" />
				<span className="absolute inset-y-0 left-1/4 w-px bg-current" />
				<span className="absolute inset-y-0 left-1/2 w-px bg-current" />
				<span className="absolute inset-y-0 left-3/4 w-px bg-current" />
				<span className="absolute inset-y-0 right-0 w-px bg-current" />

				<span className="absolute left-0 top-[18%] h-px w-full bg-current opacity-60" />
				<span className="absolute left-0 top-1/2 h-px w-full bg-current opacity-70" />
				<span className="absolute bottom-[18%] left-0 h-px w-full bg-current opacity-60" />

				<span className="absolute left-0 top-1/2 size-[7px] -translate-x-1/2 -translate-y-1/2 rounded-full bg-current opacity-80" />
				<span className="absolute left-1/4 top-[18%] size-[7px] -translate-x-1/2 -translate-y-1/2 rounded-full bg-current opacity-70" />
				<span className="absolute left-1/2 top-1/2 size-[7px] -translate-x-1/2 -translate-y-1/2 rounded-full bg-current opacity-70" />
				<span className="absolute left-3/4 bottom-[18%] size-[7px] -translate-x-1/2 translate-y-1/2 rounded-full bg-current opacity-70" />
				<span className="absolute right-0 top-1/2 size-[7px] -translate-y-1/2 translate-x-1/2 rounded-full bg-current opacity-80" />
			</div>
		</div>
	);
}

export default EditorialGrid;
