"use client";

import { useEffect, useRef } from "react";
import Link from "next/link";
import { usePrefersReducedMotion } from "@/shared/hooks/usePrefersReducedMotion";
import { knowledgeDepartments as departments, knowledgeGraphFallbackPath } from "./knowledgeGraphData";
import styles from "./KnowledgeCards.module.css";

const fallbackPath = knowledgeGraphFallbackPath();

export function KnowledgeGraph({ expanded = false }: { expanded?: boolean }) {
	const hostRef = useRef<HTMLDivElement>(null);
	const reducedMotion = usePrefersReducedMotion();
	useEffect(() => {
		const host = hostRef.current;
		if (!host) return;
		let cancelled = false;
		let dispose: (() => void) | undefined;
		const observer = new IntersectionObserver(([entry]) => {
			if (!entry.isIntersecting) return;
			observer.disconnect();
			void import("./knowledgeGraphScene").then(({ mountKnowledgeGraph }) => {
				if (!cancelled) dispose = mountKnowledgeGraph(host, reducedMotion);
			}).catch(() => { /* The static graph and department nodes remain usable without WebGL. */ });
		}, { rootMargin: "240px" });
		observer.observe(host);
		return () => { cancelled = true; observer.disconnect(); dispose?.(); };
	}, [reducedMotion]);

	return (
		<div className={styles.graph} data-expanded={expanded} ref={hostRef}>
			<svg aria-hidden="true" className={styles.graphFallback} viewBox="0 0 300 300" fill="none">
				<path d={fallbackPath} stroke="#625f55" strokeWidth="0.4" opacity="0.2" />
			</svg>
			<div className={styles.departmentNodes}>
				{departments.map((department, index) => (
					<Link
						aria-label={`Se hvordan ${department.name} bruker felles kunnskap`}
						className={styles.departmentNode}
						data-business-node={department.name}
						data-node-x={department.x}
						data-node-y={department.y}
						data-highlight={index === 0}
						key={department.name}
						style={{ left: `${50 + department.x * 34}%`, top: `${50 - department.y * 34}%` }}
						href="/plattform/felles-kontekst#kilder"
					>
						<span className={styles.departmentDot} />
						<span className={styles.departmentLabel}>{department.name}</span>
					</Link>
				))}
			</div>
		</div>
	);
}
