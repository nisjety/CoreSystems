"use client";

import { useEffect, useRef, useState } from "react";
import { usePrefersReducedMotion } from "@/shared/hooks/usePrefersReducedMotion";
import { knowledgeDepartments as departments, knowledgeGraphFallbackPath } from "./knowledgeGraphData";
import styles from "./KnowledgeCards.module.css";

const fallbackPath = knowledgeGraphFallbackPath();

export function KnowledgeGraph({ expanded = false }: { expanded?: boolean }) {
	const hostRef = useRef<HTMLDivElement>(null);
	const [selected, setSelected] = useState<string | null>(null);
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
		<div className={styles.graph} data-expanded={expanded} data-selected-node={expanded ? selected ?? "" : ""} ref={hostRef}>
			<svg aria-hidden="true" className={styles.graphFallback} viewBox="0 0 300 300" fill="none">
				<path d={fallbackPath} stroke="#625f55" strokeWidth="0.4" opacity="0.2" />
			</svg>
			<div aria-label="Eksempel på avdelinger i en virksomhet" aria-hidden={!expanded} className={styles.departmentNodes} inert={!expanded}>
				{departments.map((department, index) => (
					<button
						aria-label={department.name}
						aria-expanded={selected === department.name}
						aria-controls={`department-preview-${index}`}
						aria-describedby={selected === department.name ? `department-preview-${index}` : undefined}
						className={styles.departmentNode}
						data-business-node={department.name}
						data-node-x={department.x}
						data-node-y={department.y}
						data-highlight={index === 0}
						data-selected={selected === department.name}
						key={department.name}
						onClick={() => setSelected(selected === department.name ? null : department.name)}
						style={{ left: `${50 + department.x * 34}%`, top: `${50 - department.y * 34}%` }}
						tabIndex={expanded ? 0 : -1}
						type="button"
					>
						<span className={styles.departmentDot} />
						<span className={styles.departmentLabel}>{department.name}</span>
						<span className={styles.departmentPreview} id={`department-preview-${index}`} aria-hidden={selected !== department.name}>
							<span className={styles.departmentPreviewHeading}>{department.name}<small>Eksempel</small></span>
							{department.items.map((item) => <span className={styles.departmentItem} key={item}><span aria-hidden="true">≡</span>{item}</span>)}
						</span>
					</button>
				))}
			</div>
		</div>
	);
}
