"use client";

import dynamic from "next/dynamic";
import { useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import type { SenseKind } from "./SenseScene";
import { VerevonMark } from "../sections/VerevonMark";
import styles from "./SensesMotion.module.css";

const descriptions: Record<SenseKind, string> = {
	delegate: "Et signal settes i sammenheng med relevant kunnskap og systemdata. Arbeid som følger virksomhetens regler kan gå videre, mens vurderinger går til riktig person.",
	learn: "Tidligere vurderinger, dokumenter og arbeidsmåter gir nye saker den konteksten virksomheten allerede har.",
	oversee: "Én arbeidsflate viser grunnlag, ansvar og forslag. Når noe krever en beslutning, venter Verevon på den riktige personen.",
};

const captions: Record<SenseKind, string> = {
	delegate: "Fra signal til styrt arbeid",
	learn: "Kunnskap som følger arbeidet",
	oversee: "Felles oversikt. Kontroll ved behov.",
};

function backgroundStyle(backgroundSrc: string): CSSProperties {
	return { "--sense-background": `url("${backgroundSrc}")` } as CSSProperties;
}

function Placeholder({ backgroundSrc }: { backgroundSrc: string }) {
	return <div aria-hidden="true" className={styles.placeholder} style={backgroundStyle(backgroundSrc)}><span><VerevonMark style={{ display: "block", width: "100%", height: "100%", transform: "scale(1.3)" }} /></span></div>;
}

const defaultBackground = "/verevon-senses/glass-study.webp";
const SensePlayer = dynamic(() => import("./SensePlayer"), { ssr: false, loading: () => <Placeholder backgroundSrc={defaultBackground} /> });

export function SensesMotion({ kind, backgroundSrc = defaultBackground }: { kind: SenseKind; backgroundSrc?: string }) {
	const ref = useRef<HTMLElement>(null);
	const [ready, setReady] = useState(false);
	useEffect(() => {
		const observer = new IntersectionObserver(([entry]) => {
			if (entry.isIntersecting) { setReady(true); observer.disconnect(); }
		}, { rootMargin: "500px" });
		if (ref.current) observer.observe(ref.current);
		return () => observer.disconnect();
	}, []);

	return <figure className={styles.figure} ref={ref}>
		<div className={styles.frame} data-sense-kind={kind}>{ready ? <SensePlayer kind={kind} backgroundSrc={backgroundSrc} /> : <Placeholder backgroundSrc={backgroundSrc} />}</div>
		<figcaption className={styles.caption}><span>{captions[kind]}</span><span className="sr-only">{descriptions[kind]}</span></figcaption>
	</figure>;
}
