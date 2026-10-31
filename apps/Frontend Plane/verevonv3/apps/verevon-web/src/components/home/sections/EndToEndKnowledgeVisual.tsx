"use client";

import Image from "next/image";
import Link from "next/link";
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { usePrefersReducedMotion } from "@/shared/hooks/usePrefersReducedMotion";
import { KnowledgeGraph } from "./KnowledgeGraph";
import {
	SYSTEM_FIELD_ITEMS,
	getSystemFieldSelection,
} from "./brand-connector-data";
import { DEFAULT_CARD_INTEGRATIONS } from "@/components/integrations/integration-catalog";
import styles from "./KnowledgeCards.module.css";

export type KnowledgeCardVisual = "business" | "connect" | "knowledge";

export function ConnectVisual({ expanded = false }: { expanded?: boolean }) {
	const [selected, setSelected] = useState<string | null>(null);
	const tiles = [null, 0, null, 1, null, 2, 3, null, 4, null, 5, null, null, 6, null, 7, null, 8, 9, null, 10, null, 11, null];
	return (
		<div className={styles.connectionScene} data-expanded={expanded}>
			<div aria-hidden="true" className={styles.connectors}>
				<div className={styles.tiles}>
					{tiles.map((iconIndex, index) => (
						<div className={styles.tile} key={index}>
							{iconIndex !== null && (
								DEFAULT_CARD_INTEGRATIONS[iconIndex].logo ? (
									<Image alt="" height={28} src={`/brand-logos/${DEFAULT_CARD_INTEGRATIONS[iconIndex].logo}`} width={28} />
								) : <span aria-hidden="true" className={styles.tileMark}>{DEFAULT_CARD_INTEGRATIONS[iconIndex].mark}</span>
							)}
						</div>
					))}
				</div>
			</div>
			<div aria-hidden={!expanded} className={styles.systems} inert={!expanded}>
				<p className={styles.systemsCaption} aria-live="polite">{getSystemFieldSelection(selected)}</p>
				<div className={styles.systemField}>
					<div aria-label="Systemer i arbeidshverdagen" className={styles.systemGrid} role="group">
						{SYSTEM_FIELD_ITEMS.map((system, index) => (
							<Link
								aria-label={`Utforsk ${system.name} i integrasjonskatalogen`}
								className={styles.system}
								data-context={system.kind}
								data-integration-id={system.id}
								data-selected={selected === system.name}
								data-system-link=""
								data-system={system.name}
								href={`/integrasjoner?integration=${system.id}#katalog`}
								key={system.name}
								onFocus={() => setSelected(system.name)}
								onPointerEnter={(event) => { if (event.pointerType === "mouse") setSelected(system.name); }}
								style={{ "--tile-order": index } as CSSProperties}
							>
								{system.logo ? <Image alt="" src={`/brand-logos/${system.logo}`} width={52} height={36} /> : <span aria-hidden="true" className={styles.systemMark}>{system.mark}</span>}
								<span className={styles.systemName}>{system.name}</span>
							</Link>
						))}
					</div>
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
