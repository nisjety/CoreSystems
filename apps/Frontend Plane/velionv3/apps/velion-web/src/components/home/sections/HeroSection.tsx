import Image from "next/image";
import { ArrowButton } from "@/components/ui/ArrowButton";

export function HeroSection() {
	return (
		<section
			className="velion-hero-load relative isolate min-h-svh overflow-hidden bg-background text-velion-c-white"
			data-hero-parallax=""
			id="top"
		>
			<div aria-hidden="true" className="absolute inset-0 z-0 bg-background" />

			<div
				aria-hidden="true"
				className="absolute inset-x-0 -top-[8%] -bottom-[18%] z-10 overflow-hidden will-change-transform"
				data-hero-parallax-media=""
				data-parallax-effect=""
				data-parallax-options='{"from":{"y":"0%"},"to":{"y":"80%"},"start":"top top","end":"bottom top","disableOnMobile":false,"disableOnTablet":false}'
			>
				<div
					className="velion-hero-load__frame absolute inset-0 overflow-hidden bg-background"
					data-hero-load-frame=""
				>
					<video
						aria-hidden="true"
						autoPlay
						className="velion-hero-load__intro absolute inset-0 size-full object-cover object-center"
						data-hero-load-intro-video=""
						muted
						playsInline
						poster="/velion-vibe/hero-eye-source-poster.jpg"
						preload="auto"
						src="/velion-vibe/hero-eye-source.mp4"
					/>

					<div
						className="absolute inset-0 opacity-0"
						data-hero-load-final-media=""
					>
						<Image
							alt=""
							className="absolute inset-0 size-full scale-[1.05] object-cover object-[48%_48%] brightness-[0.64] contrast-[1.1] saturate-[0.86]"
							data-hero-load-final-image=""
							fill
							priority
							sizes="100vw"
							src="/velion-vibe/human-haze.png"
						/>

						<div
							className="velion-hero-load__ambient absolute inset-0"
							data-hero-load-final-overlay=""
						/>
						<div
							className="velion-hero-load__organic-shadows absolute inset-0"
							data-hero-load-final-overlay=""
						/>
						<div
							className="velion-hero-load__fabric absolute inset-0"
							data-hero-load-final-overlay=""
						/>
					</div>
				</div>
			</div>

			<div
				className="absolute bottom-0 left-[clamp(56px,5.55vw,208px)] top-0 z-20 flex w-[min(690px,calc(100%-clamp(56px,5.55vw,208px)*2))] max-w-[690px] flex-col items-start justify-center will-change-transform max-[760px]:left-[clamp(24px,4vw,56px)] max-[760px]:right-[clamp(24px,4vw,56px)] max-[760px]:w-auto max-[760px]:max-w-none"
				data-hero-parallax-content=""
				data-parallax-effect=""
				data-parallax-options='{"from":{"y":"0%"},"to":{"y":"40%"},"start":"top top","end":"bottom top","disableOnMobile":false,"disableOnTablet":false}'
			>
				<p
					className="mb-6 font-protokoll text-[0.82rem] font-medium uppercase leading-none tracking-[0.18em] text-[color-mix(in_srgb,var(--velion-c-white)_62%,transparent)] max-[760px]:mb-5 max-[760px]:text-[0.76rem]"
					data-hero-load-copy-item=""
				>
					Den norske AI-arbeidsbenken
				</p>

				<h1
					className="mb-9 max-w-[680px] font-arbeit text-[3.15rem] font-light leading-[0.98] tracking-normal text-velion-c-white sm:text-[4.7rem] lg:text-[6.7rem] 2xl:text-[7.8rem] max-[760px]:mb-6"
					data-hero-load-copy-item=""
				>
					AI som handler — forankret og godkjent.
				</h1>

				<p
					className="mb-[34px] max-w-[500px] font-protokoll text-[1.08rem] font-light leading-[1.5] text-[color-mix(in_srgb,var(--velion-c-white)_78%,transparent)] sm:text-[1.16rem] max-[760px]:max-w-[420px] max-[760px]:text-base"
					data-hero-load-copy-item=""
				>
					Velion gjør forankret innsikt til godkjent handling. Hvert svar er
					kildebelagt, hvert steg venter på et menneske — og dataene blir i EU
					som standard.
				</p>

				<div
					className="flex w-fit flex-wrap items-center gap-x-9 gap-y-4"
					data-hero-load-copy-item=""
				>
					<ArrowButton href="#produkt" variant="light">
						Se hvordan det virker
					</ArrowButton>

					<a
						className="font-protokoll text-[0.98rem] font-light text-[color-mix(in_srgb,var(--velion-c-white)_70%,transparent)] underline-offset-[6px] transition-colors hover:text-velion-c-white hover:underline"
						href="/trust"
					>
						Tillit &amp; datasuverenitet
					</a>
				</div>
			</div>

			<a
				className="absolute bottom-[clamp(42px,6vh,76px)] left-[clamp(56px,5.55vw,208px)] z-20 inline-flex items-center gap-3 text-[1.06rem] text-[color-mix(in_srgb,var(--velion-c-white)_88%,transparent)] max-[760px]:bottom-[34px] max-[760px]:left-[clamp(24px,4vw,56px)]"
				data-hero-load-copy-item=""
				href="#produkt"
			>
				<svg
					aria-hidden="true"
					className="size-[22px] fill-none stroke-current stroke-[1.8]"
					viewBox="0 0 18 18"
				>
					<path d="m3 6 6 6 6-6" />
				</svg>
				<span>Bla ned</span>
			</a>
		</section>
	);
}
