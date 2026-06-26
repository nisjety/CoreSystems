import React from "react";
import { cn } from "@/lib/utils";

export interface CpuArchitectureSvgProps {
	animateLines?: boolean;
	animateMarkers?: boolean;
	animateText?: boolean;
	className?: string;
	height?: string;
	lineMarkerSize?: number;
	showCpuConnections?: boolean;
	text?: string;
	width?: string;
}

const pathDurations = [
	"1.6s",
	"1.6s",
	"1.6s",
	"1.6s",
	"1.6s",
	"1.6s",
	"1.6s",
	"1.6s",
];

export function CpuArchitecture({
	animateLines = true,
	animateMarkers = true,
	animateText = true,
	className,
	height = "100%",
	lineMarkerSize = 18,
	showCpuConnections = true,
	text = "CPU",
	width = "100%",
}: CpuArchitectureSvgProps) {
	return (
		<svg
			className={cn("text-muted", className)}
			height={height}
			viewBox="0 0 200 100"
			width={width}
			xmlns="http://www.w3.org/2000/svg"
		>
			<g
				fill="none"
				markerStart="url(#cpu-circle-marker)"
				pathLength="100"
				stroke="currentColor"
				strokeDasharray="100 100"
				strokeWidth="0.4"
			>
				<path
					d="M 10 20 h 79.5 q 5 0 5 5 v 30"
					pathLength="100"
					strokeDasharray="100 100"
				/>
				<path
					d="M 180 10 h -69.7 q -5 0 -5 5 v 30"
					pathLength="100"
					strokeDasharray="100 100"
				/>
				<path d="M 130 20 v 21.8 q 0 5 -5 5 h -10" />
				<path d="M 170 80 v -21.8 q 0 -5 -5 -5 h -50" />
				<path
					d="M 135 65 h 15 q 5 0 5 5 v 10 q 0 5 -5 5 h -39.8 q -5 0 -5 -5 v -20"
					pathLength="100"
					strokeDasharray="100 100"
				/>
				<path d="M 94.8 95 v -36" />
				<path d="M 88 88 v -15 q 0 -5 -5 -5 h -10 q -5 0 -5 -5 v -5 q 0 -5 5 -5 h 14" />
				<path d="M 30 30 h 25 q 5 0 5 5 v 6.5 q 0 5 5 5 h 20" />

				{animateLines ? (
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
				) : null}
			</g>

			<defs>
				<path id="cpu-path-1" d="M 10 20 h 79.5 q 5 0 5 5 v 30" />
				<path id="cpu-path-2" d="M 180 10 h -69.7 q -5 0 -5 5 v 30" />
				<path id="cpu-path-3" d="M 130 20 v 21.8 q 0 5 -5 5 h -10" />
				<path id="cpu-path-4" d="M 170 80 v -21.8 q 0 -5 -5 -5 h -50" />
				<path id="cpu-path-5" d="M 135 65 h 15 q 5 0 5 5 v 10 q 0 5 -5 5 h -39.8 q -5 0 -5 -5 v -20" />
				<path id="cpu-path-6" d="M 94.8 95 v -36" />
				<path id="cpu-path-7" d="M 88 88 v -15 q 0 -5 -5 -5 h -10 q -5 0 -5 -5 v -5 q 0 -5 5 -5 h 14" />
				<path id="cpu-path-8" d="M 30 30 h 25 q 5 0 5 5 v 6.5 q 0 5 5 5 h 20" />
			</defs>

			<g mask="url(#cpu-mask-1)">
				<circle cx="0" cy="0" fill="url(#cpu-blue-grad)" r="14">
					<animateMotion dur={pathDurations[0]} repeatCount="indefinite">
						<mpath href="#cpu-path-1" />
					</animateMotion>
				</circle>
			</g>

			<g mask="url(#cpu-mask-2)">
				<circle cx="0" cy="0" fill="url(#cpu-yellow-grad)" r="14">
					<animateMotion dur={pathDurations[1]} repeatCount="indefinite">
						<mpath href="#cpu-path-2" />
					</animateMotion>
				</circle>
			</g>

			<g mask="url(#cpu-mask-3)">
				<circle cx="0" cy="0" fill="url(#cpu-pinkish-grad)" r="14">
					<animateMotion dur={pathDurations[2]} repeatCount="indefinite">
						<mpath href="#cpu-path-3" />
					</animateMotion>
				</circle>
			</g>

			<g mask="url(#cpu-mask-4)">
				<circle cx="0" cy="0" fill="url(#cpu-white-grad)" r="14">
					<animateMotion dur={pathDurations[3]} repeatCount="indefinite">
						<mpath href="#cpu-path-4" />
					</animateMotion>
				</circle>
			</g>

			<g mask="url(#cpu-mask-5)">
				<circle cx="0" cy="0" fill="url(#cpu-green-grad)" r="14">
					<animateMotion dur={pathDurations[4]} repeatCount="indefinite">
						<mpath href="#cpu-path-5" />
					</animateMotion>
				</circle>
			</g>

			<g mask="url(#cpu-mask-6)">
				<circle cx="0" cy="0" fill="url(#cpu-orange-grad)" r="14">
					<animateMotion dur={pathDurations[5]} repeatCount="indefinite">
						<mpath href="#cpu-path-6" />
					</animateMotion>
				</circle>
			</g>

			<g mask="url(#cpu-mask-7)">
				<circle cx="0" cy="0" fill="url(#cpu-cyan-grad)" r="14">
					<animateMotion dur={pathDurations[6]} repeatCount="indefinite">
						<mpath href="#cpu-path-7" />
					</animateMotion>
				</circle>
			</g>

			<g mask="url(#cpu-mask-8)">
				<circle cx="0" cy="0" fill="url(#cpu-rose-grad)" r="14">
					<animateMotion dur={pathDurations[7]} repeatCount="indefinite">
						<mpath href="#cpu-path-8" />
					</animateMotion>
				</circle>
			</g>

			<g>
				{showCpuConnections ? (
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
				) : null}

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
					fill={animateText ? "url(#cpu-text-gradient)" : "white"}
					fontSize="7"
					fontWeight="600"
					letterSpacing="0.05em"
					x="92"
					y="52.5"
				>
					{text}
				</text>
			</g>

			<defs>
				<mask id="cpu-mask-1">
					<path
						d="M 10 20 h 79.5 q 5 0 5 5 v 24"
						stroke="white"
						strokeLinecap="round"
						strokeLinejoin="round"
						strokeWidth="0.4"
					/>
				</mask>

				<mask id="cpu-mask-2">
					<path
						d="M 180 10 h -69.7 q -5 0 -5 5 v 24"
						stroke="white"
						strokeLinecap="round"
						strokeLinejoin="round"
						strokeWidth="0.4"
					/>
				</mask>

				<mask id="cpu-mask-3">
					<path
						d="M 130 20 v 21.8 q 0 5 -5 5 h -10"
						stroke="white"
						strokeWidth="0.4"
					/>
				</mask>

				<mask id="cpu-mask-4">
					<path
						d="M 170 80 v -21.8 q 0 -5 -5 -5 h -50"
						stroke="white"
						strokeWidth="0.4"
					/>
				</mask>

				<mask id="cpu-mask-5">
					<path
						d="M 135 65 h 15 q 5 0 5 5 v 10 q 0 5 -5 5 h -39.8 q -5 0 -5 -5 v -20"
						stroke="white"
						strokeWidth="0.4"
					/>
				</mask>

				<mask id="cpu-mask-6">
					<path
						d="M 94.8 95 v -36"
						stroke="white"
						strokeLinecap="round"
						strokeLinejoin="round"
						strokeWidth="0.4"
					/>
				</mask>

				<mask id="cpu-mask-7">
					<path
						d="M 88 88 v -15 q 0 -5 -5 -5 h -10 q -5 0 -5 -5 v -5 q 0 -5 5 -5 h 14"
						stroke="white"
						strokeLinecap="round"
						strokeLinejoin="round"
						strokeWidth="0.4"
					/>
				</mask>

				<mask id="cpu-mask-8">
					<path
						d="M 30 30 h 25 q 5 0 5 5 v 6.5 q 0 5 5 5 h 20"
						stroke="white"
						strokeLinecap="round"
						strokeLinejoin="round"
						strokeWidth="0.4"
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

				<filter id="cpu-light-shadow" height="200%" width="200%" x="-50%" y="-50%">
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
					markerHeight={lineMarkerSize}
					markerWidth={lineMarkerSize}
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
						strokeWidth="0.4"
					>
						{animateMarkers ? (
							<animate attributeName="r" dur="0.5s" values="0; 3; 2" />
						) : null}
					</circle>
				</marker>

				<linearGradient id="cpu-connection-gradient" x1="0" x2="0" y1="0" y2="1">
					<stop offset="0%" stopColor="#4F4F4F" />
					<stop offset="60%" stopColor="#121214" />
				</linearGradient>

				<linearGradient id="cpu-text-gradient" x1="0" x2="1" y1="0" y2="0">
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
							values="0; 1; 2"
						/>
					</stop>
				</linearGradient>
			</defs>
		</svg>
	);
}

export default CpuArchitecture;