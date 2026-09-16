"use client";

import Image from "next/image";
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { usePrefersReducedMotion } from "@/shared/hooks/usePrefersReducedMotion";
import { KnowledgeGraph } from "./KnowledgeGraph";
import styles from "./KnowledgeCards.module.css";

export type KnowledgeCardVisual = "business" | "connect" | "knowledge";

const integrations = ["slack.svg", "notion.svg", "outlook.svg", "sharepoint.svg", "gmail.svg", "onedrive.svg", "teams.svg", "microsoft-365.svg", "google.svg", "bring.svg", "linkedin.svg", "meta.svg"];
const systems = [
	{ name: "Visma", file: "visma", category: "ERP" },
	{ name: "Fiken", file: "fiken", category: "Regnskap" },
	{ name: "Sanity", file: "sanity", category: "Innhold" },
	{ name: "SuperOffice", file: "superoffice", category: "CRM" },
	{ name: "Bring", file: "bring", category: "Logistikk" },
	{ name: "Shopify", file: "shopify", category: "Netthandel" },
	{ name: "ShipHero", file: "shiphero", category: "Lager" },
	{ name: "ChatGPT", file: "chatgpt", category: "AI" },
];

export function ConnectVisual({ expanded = false }: { expanded?: boolean }) {
	const tiles = [null, 0, null, 1, null, 2, 3, null, 4, null, 5, null, null, 6, null, 7, null, 8, 9, null, 10, null, 11, null];
	return (
		<div className={styles.connectionScene} data-expanded={expanded}>
			<div aria-hidden="true" className={styles.connectors}>
				<div className={styles.tiles}>
					{tiles.map((iconIndex, index) => (
						<div className={styles.tile} key={index}>
							{iconIndex !== null && <Image alt="" height={28} src={`/brand-logos/${integrations[iconIndex]}`} width={28} />}
						</div>
					))}
				</div>
			</div>
			<div aria-hidden={!expanded} className={styles.systems} inert={!expanded}>
				<p className={styles.systemsCaption}>Fra ERP og CRM til resten av virksomheten</p>
				<div className={styles.systemGrid}>
					{systems.map((system, index) => (
						<div className={styles.system} key={system.file} style={{ "--tile-order": index } as CSSProperties}>
							<Image alt={system.name} src={`/brand-logos/${system.file}.svg`} width={100} height={32} />
							<span>{system.category}</span>
						</div>
					))}
				</div>
			</div>
		</div>
	);
}

export function BusinessVisual({ expanded = false }: { expanded?: boolean }) {
	const videoRef = useRef<HTMLVideoElement>(null);
	const reducedMotion = usePrefersReducedMotion();
	const [playing, setPlaying] = useState(false);
	useEffect(() => {
		const video = videoRef.current;
		if (!video) return;
		let visible = false;
		let cancelled = false;
		const sync = () => {
			if (expanded && visible && !document.hidden && !reducedMotion && !cancelled) {
				void video.play().catch(() => { /* Keep the still image if playback is unavailable. */ });
			} else video.pause();
		};
		const observer = new IntersectionObserver(([entry]) => { visible = entry.isIntersecting; sync(); }, { threshold: 0.1 });
		observer.observe(video);
		document.addEventListener("visibilitychange", sync);
		return () => {
			cancelled = true;
			observer.disconnect();
			document.removeEventListener("visibilitychange", sync);
			video.pause();
		};
	}, [expanded, reducedMotion]);
	return (
		<div aria-hidden="true" className={styles.businessMedia} data-playing={expanded && playing}>
			<Image
				alt=""
				className={styles.businessStill}
				fill
				sizes="(min-width: 1024px) 50vw, 100vw"
				src="https://images.unsplash.com/photo-1539786919-965dc728468d?auto=format&fit=crop&w=1200&q=80"
				unoptimized
			/>
			<video
				className={styles.businessVideo}
				loop muted playsInline preload="none"
				ref={videoRef}
				onPlaying={() => setPlaying(true)}
				onPause={() => setPlaying(false)}
				onError={() => setPlaying(false)}
			>
				<source src="https://videos.pexels.com/video-files/7579568/7579568-hd_1366_720_25fps.mp4" type="video/mp4" />
			</video>
			<div className={styles.businessShade} />
			<div className="absolute -right-[26%] -top-[17%] size-[93%] border border-white/20" />
			<div className="absolute -right-[4%] top-[17%] size-[55%] border border-white/14" />
			<div className="absolute right-[8%] top-[9%] h-[42%] w-px bg-verevon-coral/70" />
		</div>
	);
}

export function KnowledgeCardVisual({ type, expanded = false }: { type: KnowledgeCardVisual; expanded?: boolean }) {
	if (type === "connect") return <ConnectVisual expanded={expanded} />;
	if (type === "knowledge") return <KnowledgeGraph expanded={expanded} />;
	return <BusinessVisual expanded={expanded} />;
}
