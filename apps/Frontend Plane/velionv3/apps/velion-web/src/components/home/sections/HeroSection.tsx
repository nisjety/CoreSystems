import Image from "next/image";
import { ArrowButton } from "@/components/ui/ArrowButton";

export function HeroSection() {
	return (
		<section
			className="relative min-h-svh overflow-hidden bg-[#070809] text-velion-c-white"
			data-hero-parallax=""
			id="top"
		>
			<div
				aria-hidden="true"
				className="absolute inset-x-0 -top-[8%] -bottom-[18%] overflow-hidden bg-[#070809] will-change-transform"
				data-hero-parallax-media=""
				data-parallax-effect=""
				data-parallax-options='{"from":{"y":"0%"},"to":{"y":"80%"},"start":"top top","end":"bottom top","disableOnMobile":false,"disableOnTablet":false}'
			>
				<Image
					alt=""
					className="absolute inset-0 size-full scale-[1.04] object-cover object-[48%_48%] brightness-[0.68] contrast-[1.06] saturate-[0.84]"
					fill
					priority
					quality={95}
					sizes="100vw"
					src="/velion-vibe/human-haze.png"
				/>

				<div className="absolute inset-0 bg-[linear-gradient(90deg,rgba(4,5,6,0.92)_0%,rgba(8,9,10,0.72)_30%,rgba(8,9,10,0.16)_64%,rgba(4,5,6,0.28)_100%),linear-gradient(180deg,rgba(6,7,8,0.38),rgba(6,7,8,0.2)_48%,rgba(6,7,8,0.72)),linear-gradient(90deg,rgba(255,255,255,0.09)_1px,transparent_1px),linear-gradient(180deg,rgba(255,255,255,0.07)_1px,transparent_1px)] bg-[length:100%_100%,100%_100%,104px_104px,104px_104px]" />
			</div>

			<div
				className="absolute bottom-0 left-[clamp(56px,5.55vw,208px)] top-0 flex w-[min(650px,calc(100%-clamp(56px,5.55vw,208px)*2))] max-w-[650px] flex-col items-start justify-center will-change-transform max-[760px]:left-[clamp(24px,4vw,56px)] max-[760px]:right-[clamp(24px,4vw,56px)] max-[760px]:w-auto max-[760px]:max-w-none"
				data-hero-parallax-content=""
				data-parallax-effect=""
				data-parallax-options='{"from":{"y":"0%"},"to":{"y":"40%"},"start":"top top","end":"bottom top","disableOnMobile":false,"disableOnTablet":false}'
			>
				<p className="mb-[clamp(18px,2vw,28px)] font-protokoll text-[clamp(0.78rem,0.82vw,0.96rem)] font-medium uppercase leading-none tracking-[0.08em] text-[color-mix(in_srgb,var(--velion-c-white)_58%,transparent)]">
					Velion AI Worker
				</p>

				<h1 className="mb-[clamp(26px,3vw,42px)] max-w-[620px] font-arbeit text-[clamp(4.3rem,7.4vw,8.7rem)] font-light leading-[0.94] tracking-[-0.065em] text-velion-c-white max-[760px]:mb-6 max-[760px]:text-[3.05rem]">
					Your AI worker for customer experience.
				</h1>

				<p className="mb-[34px] max-w-[470px] font-protokoll text-[clamp(1.04rem,1.08vw,1.22rem)] font-light leading-[1.5] text-[color-mix(in_srgb,var(--velion-c-white)_76%,transparent)] max-[760px]:max-w-[420px] max-[760px]:text-base">
					Velion learns the company, drafts work, asks for approval, and keeps
					every customer action traceable.
				</p>

				<div className="flex w-fit items-center">
					<ArrowButton href="#product-loop" variant="light">
						See the system
					</ArrowButton>
				</div>
			</div>

			<a
				className="absolute bottom-[clamp(42px,6vh,76px)] left-[clamp(56px,5.55vw,208px)] inline-flex items-center gap-3 text-[1.06rem] text-[color-mix(in_srgb,var(--velion-c-white)_88%,transparent)] max-[760px]:bottom-[34px] max-[760px]:left-[clamp(24px,4vw,56px)]"
				href="#product-loop"
			>
				<svg
					aria-hidden="true"
					className="size-[22px] fill-none stroke-current stroke-[1.8]"
					viewBox="0 0 18 18"
				>
					<path d="m3 6 6 6 6-6" />
				</svg>
				<span>Scroll</span>
			</a>
		</section>
	);
}