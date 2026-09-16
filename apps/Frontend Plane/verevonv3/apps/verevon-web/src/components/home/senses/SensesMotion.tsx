"use client";

import dynamic from "next/dynamic";
import { useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import type { SenseKind } from "./SenseScene";
import styles from "./SensesMotion.module.css";

const descriptions: Record<SenseKind, string> = {
	delegate: "Verevon mottar en oppgave, finner en forfalt faktura i innboksen, kontrollerer beløpet mot regnskapet, sender oppfølgingen og oppdaterer loggen innenfor rammene dere har satt.",
	learn: "Dere viser hvordan en ukesrapport lages. Verevon husker at avvikene skal komme først og bruker tilbakemeldingen når neste rapport blir klar.",
	oversee: "Teamet følger tre oppgaver, vurderer en leverandøravtale sammen og gir klarsignal før Verevon går videre.",
};

const captions: Record<SenseKind, string> = {
	delegate: "Fra oppgave til ferdig oppfølging",
	learn: "Deres erfaring blir en del av arbeidet",
	oversee: "Felles oversikt. Menneskelig kontroll.",
};

function backgroundStyle(backgroundSrc: string): CSSProperties {
	return { "--sense-background": `url("${backgroundSrc}")` } as CSSProperties;
}

function Placeholder({ backgroundSrc }: { backgroundSrc: string }) {
	return <div aria-hidden="true"><div className={styles.placeholder} style={backgroundStyle(backgroundSrc)}><span>V</span></div><div className={styles.transport} /></div>;
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
		<div className={styles.frame}>{ready ? <SensePlayer kind={kind} backgroundSrc={backgroundSrc} /> : <Placeholder backgroundSrc={backgroundSrc} />}</div>
		<figcaption className={styles.caption}><span>{captions[kind]}</span><span className="sr-only">{descriptions[kind]}</span></figcaption>
	</figure>;
}
