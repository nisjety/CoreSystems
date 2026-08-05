export function TechnologyVisualPanel() {
	return (
		<div
			aria-label="Verevon technology visual"
			className="relative min-h-[96svh] overflow-hidden bg-[#f2f2ef] text-verevon-j-text max-[1100px]:min-h-[720px] max-[760px]:min-h-[640px]"
			role="img"
		>
			<div
				aria-hidden="true"
				className="absolute inset-0 bg-[radial-gradient(circle_at_20%_22%,rgba(255,255,255,0.72),transparent_28%),radial-gradient(circle_at_72%_52%,rgba(255,255,255,0.56),transparent_34%),linear-gradient(90deg,rgba(248,248,247,0.98)_0%,rgba(248,248,247,0.78)_35%,rgba(248,248,247,0.08)_64%,rgba(248,248,247,0.02)_100%)]"
			/>

			<div
				aria-hidden="true"
				className="absolute inset-y-[-8%] right-[-8%] w-[72%] opacity-[0.94] blur-[0.1px] max-[1100px]:right-[-24%] max-[1100px]:w-[90%] max-[760px]:right-[-46%] max-[760px]:w-[120%]"
			>
				<div className="absolute right-[8%] top-[-3%] h-[52%] w-[34%] rounded-b-[42%] bg-[linear-gradient(115deg,#ffffff_0%,#dededb_54%,#f7f7f5_100%)] shadow-[inset_26px_0_52px_rgba(23,23,23,0.055),inset_-22px_0_44px_rgba(255,255,255,0.72)]" />
				<div className="absolute right-[0%] top-[11%] h-[32%] w-[58%] rotate-[16deg] rounded-[0_0_42%_18%] bg-[linear-gradient(145deg,#ffffff_0%,#e3e3df_58%,#fafaf8_100%)] shadow-[inset_0_-18px_38px_rgba(23,23,23,0.05)]" />
				<div className="absolute right-[8%] top-[36%] h-[37%] w-[78%] rounded-[58%_0_0_42%] bg-[linear-gradient(135deg,#ffffff_0%,#e8e8e4_52%,#f9f9f7_100%)] shadow-[inset_0_18px_40px_rgba(255,255,255,0.64),inset_0_-22px_46px_rgba(23,23,23,0.055)]" />
				<div className="absolute right-[32%] top-[29%] h-[34%] w-[10%] rotate-[8deg] rounded-full bg-[linear-gradient(90deg,rgba(255,255,255,0.92),rgba(211,211,207,0.7),rgba(255,255,255,0.78))] shadow-[14px_0_28px_rgba(23,23,23,0.055)]" />

				<div className="absolute right-[39%] top-[45%] h-px w-[22%] rotate-[5deg] bg-verevon-j-text/12 blur-[0.4px]" />
				<div className="absolute right-[34%] top-[48%] size-2 rounded-full bg-verevon-j-text/80" />
				<div className="absolute right-[30%] top-[51%] size-2 rounded-full bg-verevon-j-text/80" />
				<div className="absolute right-[26%] top-[54%] size-2 rounded-full bg-verevon-j-text/80" />
			</div>

			<div className="relative z-[2] flex min-h-[96svh] flex-col justify-between px-[clamp(28px,4vw,72px)] pb-[clamp(42px,5vw,74px)] pt-[clamp(56px,7vw,92px)] max-[1100px]:min-h-[720px] max-[760px]:min-h-[640px]">
				<div>
					<h2 className="m-0 max-w-[760px] font-arbeit text-[clamp(3.2rem,4.6vw,6rem)] font-light leading-[0.96] tracking-[-0.07em] text-verevon-j-text/20">
						Pioneering technology
					</h2>

					<p className="m-0 mt-7 max-w-[560px] font-protokoll text-[clamp(1rem,1vw,1.18rem)] font-light leading-[1.45] text-verevon-j-text/38">
						Verevon turns monitored signals into source-backed briefs,
						reviewable actions, and auditable execution paths.
					</p>
				</div>

				<div className="grid w-full max-w-[980px] grid-cols-3 gap-[clamp(18px,2vw,34px)] max-[760px]:grid-cols-1">
					<div>
						<strong className="font-arbeit text-[clamp(2.1rem,2.2vw,3.1rem)] font-light leading-none tracking-[-0.055em] text-verevon-j-text/88">
							<span className="mr-2 text-[0.82em]">›</span>4{" "}
							<sup className="relative top-[-0.14em] font-protokoll text-[0.38em] font-light tracking-normal text-verevon-j-text/72">
								steps
							</sup>
						</strong>
						<span className="mt-4 block font-protokoll text-[clamp(0.95rem,0.95vw,1.08rem)] font-light leading-[1.3] text-verevon-j-text/48">
							AI worker loop
						</span>
						<span className="mt-8 block h-px w-full bg-verevon-j-text/18" />
					</div>

					<div>
						<strong className="font-arbeit text-[clamp(2.1rem,2.2vw,3.1rem)] font-light leading-none tracking-[-0.055em] text-verevon-j-text/88">
							100{" "}
							<sup className="relative top-[-0.14em] font-protokoll text-[0.38em] font-light tracking-normal text-verevon-j-text/72">
								%
							</sup>
						</strong>
						<span className="mt-4 block font-protokoll text-[clamp(0.95rem,0.95vw,1.08rem)] font-light leading-[1.3] text-verevon-j-text/48">
							Manual parity
						</span>
						<span className="mt-8 block h-px w-full bg-verevon-j-text/18" />
					</div>

					<div>
						<strong className="font-arbeit text-[clamp(2.1rem,2.2vw,3.1rem)] font-light leading-none tracking-[-0.055em] text-verevon-j-text/88">
							HITL{" "}
							<sup className="relative top-[-0.14em] font-protokoll text-[0.38em] font-light tracking-normal text-verevon-j-text/72">
								approved
							</sup>
						</strong>
						<span className="mt-4 block font-protokoll text-[clamp(0.95rem,0.95vw,1.08rem)] font-light leading-[1.3] text-verevon-j-text/48">
							Risky actions
						</span>
						<span className="mt-8 block h-px w-full bg-verevon-j-text/18" />
					</div>
				</div>
			</div>

			<div className="pointer-events-none absolute bottom-[18%] right-[8%] z-[3] w-[min(34vw,560px)] opacity-[0.34] mix-blend-multiply max-[1100px]:bottom-[30%] max-[1100px]:right-[8%] max-[1100px]:w-[min(54vw,560px)] max-[760px]:hidden">
				<div className="rounded-xl bg-white/20 p-4">
					<svg
						className="text-verevon-j-text/55"
						height="100%"
						viewBox="0 0 200 100"
						width="100%"
					>
						<style>
							{`
								.cpu-architecture {
									offset-anchor: 10px 0;
									animation-name: verevon-cpu-animation-path;
									animation-iteration-count: infinite;
									animation-timing-function: cubic-bezier(0.75, -0.01, 0, 0.99);
									opacity: 1;
									filter: saturate(1.6) drop-shadow(0 0 10px currentColor);
									transform-box: fill-box;
									transform-origin: center;
									will-change: offset-distance;
								}

								.cpu-line-1 {
									offset-path: path("M 10 20 h 79.5 q 5 0 5 5 v 30");
									animation-delay: 1s;
									animation-duration: 5s;
								}

								.cpu-line-2 {
									offset-path: path("M 180 10 h -69.7 q -5 0 -5 5 v 40");
									animation-delay: 6s;
									animation-duration: 2s;
								}

								.cpu-line-3 {
									offset-path: path("M 130 20 v 21.8 q 0 5 -5 5 h -25");
									animation-delay: 4s;
									animation-duration: 6s;
								}

								.cpu-line-4 {
									offset-path: path("M 170 80 v -21.8 q 0 -5 -5 -5 h -65");
									animation-delay: 3s;
									animation-duration: 3s;
								}

								.cpu-line-5 {
									offset-path: path("M 135 65 h 15 q 5 0 5 5 v 10 q 0 5 -5 5 h -39.8 q -5 0 -5 -5 v -35");
									animation-delay: 9s;
									animation-duration: 4s;
								}

								.cpu-line-6 {
									offset-path: path("M 94.8 95 v -46");
									animation-delay: 3s;
									animation-duration: 7s;
								}

								.cpu-line-7 {
									offset-path: path("M 88 88 v -15 q 0 -5 -5 -5 h -10 q -5 0 -5 -5 v -5 q 0 -5 5 -5 h 28");
									animation-delay: 4s;
									animation-duration: 4s;
								}

								.cpu-line-8 {
									offset-path: path("M 30 30 h 25 q 5 0 5 5 v 6.5 q 0 5 5 5 h 35");
									animation-delay: 3s;
									animation-duration: 3s;
								}

								@keyframes verevon-cpu-animation-path {
									from {
										offset-distance: 0%;
									}

									to {
										offset-distance: 100%;
									}
								}
							`}
						</style>

						<g
							fill="none"
							markerStart="url(#cpu-circle-marker)"
							pathLength={100}
							stroke="currentColor"
							strokeDasharray="100 100"
							strokeWidth="0.3"
						>
							<path
								d="M 10 20 h 79.5 q 5 0 5 5 v 30"
								pathLength={100}
								strokeDasharray="100 100"
							/>
							<path
								d="M 180 10 h -69.7 q -5 0 -5 5 v 30"
								pathLength={100}
								strokeDasharray="100 100"
							/>
							<path d="M 130 20 v 21.8 q 0 5 -5 5 h -10" />
							<path d="M 170 80 v -21.8 q 0 -5 -5 -5 h -50" />
							<path
								d="M 135 65 h 15 q 5 0 5 5 v 10 q 0 5 -5 5 h -39.8 q -5 0 -5 -5 v -20"
								pathLength={100}
								strokeDasharray="100 100"
							/>
							<path d="M 94.8 95 v -36" />
							<path d="M 88 88 v -15 q 0 -5 -5 -5 h -10 q -5 0 -5 -5 v -5 q 0 -5 5 -5 h 14" />
							<path d="M 30 30 h 25 q 5 0 5 5 v 6.5 q 0 5 5 5 h 20" />
							<animate
								attributeName="stroke-dashoffset"
								calcMode="spline"
								dur="1s"
								fill="freeze"
								from="100"
								keySplines="0.25,0.1,0.5,1"
								keyTimes="0; 1"
								to="0"
							/>
						</g>

						<g mask="url(#cpu-mask-1)">
							<circle
								className="cpu-architecture cpu-line-1"
								cx="0"
								cy="0"
								fill="url(#cpu-blue-grad)"
								r="8"
							/>
						</g>
						<g mask="url(#cpu-mask-2)">
							<circle
								className="cpu-architecture cpu-line-2"
								cx="0"
								cy="0"
								fill="url(#cpu-yellow-grad)"
								r="8"
							/>
						</g>
						<g mask="url(#cpu-mask-3)">
							<circle
								className="cpu-architecture cpu-line-3"
								cx="0"
								cy="0"
								fill="url(#cpu-pinkish-grad)"
								r="8"
							/>
						</g>
						<g mask="url(#cpu-mask-4)">
							<circle
								className="cpu-architecture cpu-line-4"
								cx="0"
								cy="0"
								fill="url(#cpu-white-grad)"
								r="8"
							/>
						</g>
						<g mask="url(#cpu-mask-5)">
							<circle
								className="cpu-architecture cpu-line-5"
								cx="0"
								cy="0"
								fill="url(#cpu-green-grad)"
								r="8"
							/>
						</g>
						<g mask="url(#cpu-mask-6)">
							<circle
								className="cpu-architecture cpu-line-6"
								cx="0"
								cy="0"
								fill="url(#cpu-orange-grad)"
								r="8"
							/>
						</g>
						<g mask="url(#cpu-mask-7)">
							<circle
								className="cpu-architecture cpu-line-7"
								cx="0"
								cy="0"
								fill="url(#cpu-cyan-grad)"
								r="8"
							/>
						</g>
						<g mask="url(#cpu-mask-8)">
							<circle
								className="cpu-architecture cpu-line-8"
								cx="0"
								cy="0"
								fill="url(#cpu-rose-grad)"
								r="8"
							/>
						</g>

						<g>
							<g fill="url(#cpu-connection-gradient)">
								<rect height="5" rx="0.7" width="2.5" x="93" y="37" />
								<rect height="5" rx="0.7" width="2.5" x="104" y="37" />
								<rect
									height="5"
									rx="0.7"
									transform="rotate(90 116.25 45.5)"
									width="2.5"
									x="116.3"
									y="44"
								/>
								<rect
									height="5"
									rx="0.7"
									transform="rotate(90 116.25 45.5)"
									width="2.5"
									x="122.8"
									y="44"
								/>
								<rect
									height="5"
									rx="0.7"
									transform="rotate(180 105.25 39.5)"
									width="2.5"
									x="104"
									y="16"
								/>
								<rect
									height="5"
									rx="0.7"
									transform="rotate(180 105.25 39.5)"
									width="2.5"
									x="114.5"
									y="16"
								/>
								<rect
									height="5"
									rx="0.7"
									transform="rotate(270 115.25 19.5)"
									width="2.5"
									x="80"
									y="-13.6"
								/>
								<rect
									height="5"
									rx="0.7"
									transform="rotate(270 115.25 19.5)"
									width="2.5"
									x="87"
									y="-13.6"
								/>
							</g>
							<rect
								fill="#181818"
								filter="url(#cpu-light-shadow)"
								height="20"
								rx="2"
								width="30"
								x="85"
								y="40"
							/>
							<text
								fill="url(#cpu-text-gradient)"
								fontSize="7"
								fontWeight="600"
								letterSpacing="0.05em"
								x="92"
								y="52.5"
							>
								CPU
							</text>
						</g>

						<defs>
							<mask id="cpu-mask-1">
								<path
									d="M 10 20 h 79.5 q 5 0 5 5 v 24"
									stroke="white"
									strokeWidth="0.5"
								/>
							</mask>
							<mask id="cpu-mask-2">
								<path
									d="M 180 10 h -69.7 q -5 0 -5 5 v 24"
									stroke="white"
									strokeWidth="0.5"
								/>
							</mask>
							<mask id="cpu-mask-3">
								<path
									d="M 130 20 v 21.8 q 0 5 -5 5 h -10"
									stroke="white"
									strokeWidth="0.5"
								/>
							</mask>
							<mask id="cpu-mask-4">
								<path
									d="M 170 80 v -21.8 q 0 -5 -5 -5 h -50"
									stroke="white"
									strokeWidth="0.5"
								/>
							</mask>
							<mask id="cpu-mask-5">
								<path
									d="M 135 65 h 15 q 5 0 5 5 v 10 q 0 5 -5 5 h -39.8 q -5 0 -5 -5 v -20"
									stroke="white"
									strokeWidth="0.5"
								/>
							</mask>
							<mask id="cpu-mask-6">
								<path d="M 94.8 95 v -36" stroke="white" strokeWidth="0.5" />
							</mask>
							<mask id="cpu-mask-7">
								<path
									d="M 88 88 v -15 q 0 -5 -5 -5 h -10 q -5 0 -5 -5 v -5 q 0 -5 5 -5 h 14"
									stroke="white"
									strokeWidth="0.5"
								/>
							</mask>
							<mask id="cpu-mask-8">
								<path
									d="M 30 30 h 25 q 5 0 5 5 v 6.5 q 0 5 5 5 h 20"
									stroke="white"
									strokeWidth="0.5"
								/>
							</mask>

							<radialGradient id="cpu-blue-grad" fx="1">
								<stop offset="0%" stopColor="#00E8ED" />
								<stop offset="50%" stopColor="#08F" />
								<stop offset="100%" stopColor="transparent" />
							</radialGradient>
							<radialGradient id="cpu-yellow-grad" fx="1">
								<stop offset="0%" stopColor="#FFD800" />
								<stop offset="50%" stopColor="#FFD800" />
								<stop offset="100%" stopColor="transparent" />
							</radialGradient>
							<radialGradient id="cpu-pinkish-grad" fx="1">
								<stop offset="0%" stopColor="#830CD1" />
								<stop offset="50%" stopColor="#FF008B" />
								<stop offset="100%" stopColor="transparent" />
							</radialGradient>
							<radialGradient id="cpu-white-grad" fx="1">
								<stop offset="0%" stopColor="white" />
								<stop offset="100%" stopColor="transparent" />
							</radialGradient>
							<radialGradient id="cpu-green-grad" fx="1">
								<stop offset="0%" stopColor="#22c55e" />
								<stop offset="100%" stopColor="transparent" />
							</radialGradient>
							<radialGradient id="cpu-orange-grad" fx="1">
								<stop offset="0%" stopColor="#f97316" />
								<stop offset="100%" stopColor="transparent" />
							</radialGradient>
							<radialGradient id="cpu-cyan-grad" fx="1">
								<stop offset="0%" stopColor="#06b6d4" />
								<stop offset="100%" stopColor="transparent" />
							</radialGradient>
							<radialGradient id="cpu-rose-grad" fx="1">
								<stop offset="0%" stopColor="#f43f5e" />
								<stop offset="100%" stopColor="transparent" />
							</radialGradient>

							<filter
								height="200%"
								id="cpu-light-shadow"
								width="200%"
								x="-50%"
								y="-50%"
							>
								<feDropShadow
									dx="1.5"
									dy="1.5"
									floodColor="black"
									floodOpacity="0.1"
									stdDeviation="1"
								/>
							</filter>

							<marker
								id="cpu-circle-marker"
								markerHeight="18"
								markerWidth="18"
								refX="5"
								refY="5"
								viewBox="0 0 10 10"
							>
								<circle
									cx="5"
									cy="5"
									fill="black"
									id="innerMarkerCircle"
									r="2"
									stroke="#232323"
									strokeWidth="0.5"
								>
									<animate
										attributeName="r"
										dur="0.5s"
										values="0; 3; 2"
									/>
								</circle>
							</marker>

							<linearGradient
								id="cpu-connection-gradient"
								x1="0"
								x2="0"
								y1="0"
								y2="1"
							>
								<stop offset="0%" stopColor="#4F4F4F" />
								<stop offset="60%" stopColor="#121214" />
							</linearGradient>

							<linearGradient
								id="cpu-text-gradient"
								x1="0"
								x2="1"
								y1="0"
								y2="0"
							>
								<stop offset="0%" stopColor="#666666">
									<animate
										attributeName="offset"
										calcMode="spline"
										dur="5s"
										keySplines="0.4 0 0.2 1; 0.4 0 0.2 1"
										keyTimes="0; 0.5; 1"
										repeatCount="indefinite"
										values="-2; -1; 0"
									/>
								</stop>
								<stop offset="25%" stopColor="white">
									<animate
										attributeName="offset"
										calcMode="spline"
										dur="5s"
										keySplines="0.4 0 0.2 1; 0.4 0 0.2 1"
										keyTimes="0; 0.5; 1"
										repeatCount="indefinite"
										values="-1; 0; 1"
									/>
								</stop>
								<stop offset="50%" stopColor="#666666">
									<animate
										attributeName="offset"
										calcMode="spline"
										dur="5s"
										keySplines="0.4 0 0.2 1; 0.4 0 0.2 1"
										keyTimes="0; 0.5; 1"
										repeatCount="indefinite"
										values="0; 1; 2;"
									/>
								</stop>
							</linearGradient>
						</defs>
					</svg>
				</div>
			</div>
		</div>
	);
}

export default TechnologyVisualPanel;