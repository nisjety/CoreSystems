import type { CSSProperties } from "react";

const detailNotes = [
	"Overvåk",
	"Grunnlag",
	"Vurder",
	"Godkjenn",
	"Utfør",
	"Revider",
];

export function DetailGallerySection() {
	return (
		<section
			className="grid min-h-[100svh] grid-cols-[minmax(0,0.84fr)_minmax(320px,0.9fr)] items-center gap-[clamp(48px,7vw,140px)] bg-background px-[clamp(56px,5.55vw,208px)] py-[clamp(96px,15vh,180px)] text-verevon-j-text max-[1100px]:grid-cols-1 max-[1100px]:px-[clamp(24px,4vw,56px)]"
			id="details"
		>
			<div className="max-w-[760px]">
				<span
					className="mb-7 block font-arbeit text-[0.98rem] font-normal leading-none text-[color-mix(in_srgb,var(--verevon-a-earth)_48%,var(--verevon-text-muted))]"
					data-fade-out-top
				>
					02 / 06
				</span>

				<h1
					className="m-0 max-w-[820px] font-arbeit text-[clamp(3.35rem,6.4vw,8.6rem)] font-light leading-[0.9] tracking-[-0.078em] text-verevon-j-text"
					data-fade-out-top
				>
					Godkjenningen er en del av produktet.
				</h1>

				<div
					className="mt-[clamp(28px,3vw,46px)] max-w-[560px]"
					data-fade-out-top
				>
					<p className="m-0 font-protokoll text-[clamp(1.02rem,1.05vw,1.22rem)] font-light leading-[1.5] text-verevon-text-muted">
						Verevon kan foreslå arbeid på tvers av kundeservice, kunnskap og drift.
						Men handlinger med risiko stopper for vurdering, med kilder,
						kontekst og sporbarhet ved siden av.
					</p>
				</div>

				<div
					aria-label="Detaljert Verevon-sekvens"
					className="mt-[clamp(38px,5vw,78px)] grid max-w-[520px] grid-cols-2 border-t border-verevon-j-text/10 sm:grid-cols-3"
				>
					{detailNotes.map((note, index) => (
						<span
							className="border-b border-verevon-j-text/10 py-4 font-arbeit text-[0.82rem] font-normal uppercase leading-none tracking-[0.12em] text-verevon-j-text/55"
							key={note}
						>
							{String(index + 1).padStart(2, "0")} / {note}
						</span>
					))}
				</div>
			</div>

			<div
				aria-label="Animerte detaljpaneler"
				className="relative min-h-[620px] overflow-hidden rounded-[2px] border border-verevon-j-text/8 bg-[linear-gradient(90deg,rgba(23,23,23,0.045)_1px,transparent_1px),linear-gradient(180deg,rgba(23,23,23,0.045)_1px,transparent_1px),linear-gradient(145deg,#ffffff,#ecebea)] bg-[length:72px_72px,72px_72px,100%_100%] shadow-[inset_0_0_0_1px_rgba(255,255,255,0.32)] max-[1100px]:min-h-[520px]"
				role="img"
			>
				<div className="absolute left-[10%] top-[12%] h-[28%] w-[44%] border border-verevon-j-text/10 bg-white/42 backdrop-blur-[16px]" />
				<div className="absolute bottom-[12%] right-[10%] h-[30%] w-[48%] border border-verevon-j-text/10 bg-white/34 backdrop-blur-[18px]" />

				<div className="absolute left-[12%] top-[52%] h-px w-[72%] bg-verevon-j-text/14" />
				<div className="absolute left-[22%] top-[34%] h-px w-[48%] rotate-[-18deg] bg-verevon-j-text/14" />
				<div className="absolute left-[68%] top-[18%] h-[64%] w-px bg-verevon-j-text/14" />

				{detailNotes.map((note, index) => (
					<span
						aria-hidden="true"
						className={[
							"absolute block overflow-hidden border border-verevon-j-text/10 bg-white/48 shadow-[0_10px_28px_rgba(23,23,23,0.035)] backdrop-blur-[14px]",
							"animate-[verevon-detail-frame_18s_ease-in-out_infinite]",
							index === 0
								? "left-[12%] top-[18%] h-[22%] w-[32%]"
								: "",
							index === 1
								? "left-[46%] top-[10%] h-[18%] w-[38%]"
								: "",
							index === 2
								? "left-[20%] top-[42%] h-[20%] w-[30%]"
								: "",
							index === 3
								? "left-[58%] top-[42%] h-[24%] w-[28%]"
								: "",
							index === 4
								? "left-[10%] bottom-[12%] h-[18%] w-[38%]"
								: "",
							index === 5
								? "right-[10%] bottom-[10%] h-[20%] w-[34%]"
								: "",
						]
							.filter(Boolean)
							.join(" ")}
						key={note}
						style={{ "--delay": `${index * 3}s` } as CSSProperties}
					>
						<span className="absolute left-4 top-4 font-arbeit text-[0.68rem] font-normal uppercase tracking-[0.16em] text-verevon-j-text/36">
							{note}
						</span>
						<span className="absolute bottom-4 left-4 right-4 h-px bg-verevon-j-text/12" />
						<span className="absolute bottom-7 left-4 h-px w-[46%] bg-verevon-j-text/10" />
					</span>
				))}

				<span className="absolute left-[18%] top-[36%] size-[13px] rounded-full border border-verevon-j-text/25 bg-background shadow-[0_0_0_6px_rgba(248,248,247,0.64)]" />
				<span className="absolute left-[67%] top-[30%] size-[13px] rounded-full border border-verevon-j-text/25 bg-background shadow-[0_0_0_6px_rgba(248,248,247,0.64)]" />
				<span className="absolute left-[52%] top-[66%] size-[13px] rounded-full border border-verevon-a-earth/40 bg-background shadow-[0_0_0_6px_rgba(248,248,247,0.64)]" />
			</div>
		</section>
	);
}

export default DetailGallerySection;
