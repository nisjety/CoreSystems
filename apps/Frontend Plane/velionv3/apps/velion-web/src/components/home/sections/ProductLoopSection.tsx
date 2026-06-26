import type { CSSProperties } from "react";
import { ArrowButton } from "@/components/ui/ArrowButton";

const loopStates = ["Spør", "Utkast", "Godkjenn", "Revider"];

export function ProductLoopSection() {
	return (
		<section
			className="relative grid min-h-[min(880px,96svh)] grid-cols-[minmax(520px,0.84fr)_minmax(0,1.16fr)] overflow-hidden border-y border-velion-j-text/8 bg-background text-velion-j-text max-[1100px]:grid-cols-1"
			data-product-loop
			id="flyt"
		>
			<div className="grid max-w-[620px] content-center px-[clamp(56px,5.55vw,208px)] py-[clamp(80px,8.4vw,124px)] pr-[clamp(34px,4.6vw,72px)] max-[1100px]:max-w-none max-[1100px]:px-[clamp(24px,4vw,56px)]">
				<a
					className="fade-out-top inline-flex w-fit items-center font-protokoll text-[clamp(0.95rem,0.9vw,1.05rem)] font-light leading-[1.18] text-[color-mix(in_srgb,var(--velion-a-earth)_78%,var(--velion-j-text))] transition-colors hover:text-velion-j-text"
					data-fade-out-top
					href="#kontakt"
				>
					Tidlig tilgang: suveren AI-arbeidsbenk
					<span
						aria-hidden="true"
						className="ml-3 inline-block w-[clamp(52px,5.8vw,86px)] translate-y-[-1px] overflow-hidden whitespace-nowrap"
					>
						-&gt;
					</span>
				</a>

				<h1
					className="fade-out-top m-0 mt-[clamp(28px,3.2vw,44px)] max-w-[540px] font-arbeit text-[clamp(3.35rem,6.2vw,8.2rem)] font-light leading-[0.9] tracking-[-0.078em] text-velion-j-text"
					data-fade-out-top
				>
					Kjør kundearbeid på autopilot.
				</h1>

				<div
					className="fade-out-top mt-[clamp(26px,2.8vw,40px)] max-w-[520px]"
					data-fade-out-top
				>
					<p className="m-0 font-protokoll text-[clamp(1.02rem,1.05vw,1.22rem)] font-light leading-[1.5] text-velion-text-muted">
						Spør på naturlig språk. Velion kan skrive utkast, bygge
						arbeidsflyter, stoppe for godkjenning og holde kildesporet synlig
						før noen handling når en kunde.
					</p>
				</div>

				<div
					className="fade-out-top mt-[clamp(42px,5vw,72px)]"
					data-fade-out-top
				>
					<ArrowButton href="#plattform" variant="dark">
						Følg arbeidssløyfen
					</ArrowButton>
				</div>
			</div>

			<div
				aria-label="Animerte Velion-skjermbilder"
				className="relative min-h-[min(880px,96svh)] min-w-0 overflow-hidden border-l border-velion-j-text/8 bg-[linear-gradient(90deg,rgba(255,255,255,0.24),rgba(255,255,255,0)),color-mix(in_srgb,var(--velion-bg-soft)_92%,var(--velion-a-earth))] max-[1100px]:min-h-[620px] max-[1100px]:border-l-0 max-[1100px]:border-t"
			>
				<div
					aria-hidden="true"
					className="absolute inset-0 bg-[radial-gradient(circle_at_72%_24%,rgba(238,122,80,0.08),transparent_24%),radial-gradient(circle_at_18%_76%,rgba(41,64,74,0.08),transparent_26%),linear-gradient(90deg,rgba(23,23,23,0.035)_1px,transparent_1px),linear-gradient(180deg,rgba(23,23,23,0.03)_1px,transparent_1px)] bg-[length:100%_100%,100%_100%,92px_92px,92px_92px]"
				/>

				<div className="absolute inset-[clamp(74px,8.4vw,128px)_var(--velion-edge)_clamp(58px,6.6vw,96px)_clamp(42px,4.5vw,72px)] grid grid-rows-[minmax(0,1fr)_auto] content-center gap-[clamp(14px,1.4vw,20px)] max-[1100px]:inset-[clamp(34px,6vw,72px)_clamp(24px,4vw,56px)]">
					<div className="relative aspect-[1.818] w-full max-w-[1040px] self-center justify-self-end overflow-hidden rounded-l-[18px] border border-[rgba(31,31,29,0.1)] bg-white/85 shadow-[0_48px_140px_rgba(31,31,29,0.13),inset_0_1px_0_rgba(255,255,255,0.82)] max-[1100px]:justify-self-center max-[1100px]:rounded-[18px]">
						<video
							aria-label="Opptak av Velion-dashbordet der norske instruksjoner skrives inn i komponisten."
							autoPlay
							className="block h-full w-full object-cover object-center"
							loop
							muted
							playsInline
							poster="/velion-product-shots/dashboard-expanded-prompt.png"
							preload="metadata"
						>
							<source
								src="/velion-product-shots/velion-dashboard-typing.mp4"
								type="video/mp4"
							/>
						</video>
					</div>

					<div
						aria-label="Velion arbeidssløyfe-steg"
						className="grid w-full max-w-[760px] grid-cols-4 gap-[clamp(8px,0.8vw,12px)] justify-self-end max-[1100px]:justify-self-center max-[640px]:grid-cols-2"
					>
						{loopStates.map((state, index) => (
							<div
								className="relative overflow-hidden border border-velion-j-text/10 bg-white/48 px-4 py-3 backdrop-blur-[14px] [animation-delay:calc(var(--state-index)*120ms)]"
								key={state}
								style={{ "--state-index": index } as CSSProperties}
							>
								<span
									aria-hidden="true"
									className="absolute inset-y-0 left-0 w-[3px] bg-velion-coral/70 opacity-60"
								/>

								<span className="block font-arbeit text-[0.76rem] font-normal uppercase leading-none tracking-[0.14em] text-velion-j-text/42">
									{String(index + 1).padStart(2, "0")}
								</span>

								<span className="mt-2 block font-protokoll text-[clamp(0.92rem,0.95vw,1.08rem)] font-light leading-none text-velion-j-text/76">
									{state}
								</span>
							</div>
						))}
					</div>
				</div>
			</div>
		</section>
	);
}

export default ProductLoopSection;