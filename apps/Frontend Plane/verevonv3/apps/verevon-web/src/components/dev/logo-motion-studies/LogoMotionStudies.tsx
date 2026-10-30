"use client";

import { useState } from "react";
import { WordmarkComparison } from "./WordmarkComparison";
import styles from "./LogoMotionStudies.module.css";

const POSES = [
	{ time: 0, title: "01 · Verevon", detail: "Vår egen V. En rolig start." },
	{ time: 2050, title: "02 · Navnet ruller frem", detail: "Merket krymper. Bokstavene ruller opp ved siden av." },
	{ time: 2500, title: "03 · Verevon", detail: "Merket og navnet lander sammen. Logoen blir stående." },
];

export function LogoMotionStudies() {
	const [replay, setReplay] = useState(0);
	const [paused, setPaused] = useState(false);
	const [slow, setSlow] = useState(false);
	const [seekTime, setSeekTime] = useState<number>();
	const restart = () => { setReplay((current) => current + 1); setPaused(false); setSeekTime(undefined); };
	return (
		<main className={styles.page}>
			<header className={styles.intro}>
				<p className={styles.kicker}>Verevon · merkebevegelse</p>
				<h1>Merket får et navn.</h1>
				<p>B er originalen. Versjon 2 lar VEREVON rulle frem ved siden av.</p>
			</header>
			<section className={styles.compareReveal} aria-label="Sammenlign originalen og versjon 2">
				<WordmarkComparison key={replay} paused={paused} speed={slow ? 0.35 : 1} seekTime={seekTime} />
				<div className={styles.revealControls}>
					<button type="button" onClick={restart}>Spill på nytt</button>
					<button type="button" onClick={() => { setSeekTime(undefined); setPaused((current) => !current); }}>
						{paused ? "Fortsett" : "Pause"}
					</button>
					<button type="button" aria-pressed={slow} onClick={() => setSlow((current) => !current)}>
						{slow ? "Sakte film · 0,35×" : "Vis i sakte film"}
					</button>
				</div>
				<div className={styles.poses} aria-label="Se nøkkelbilder">
					{POSES.map(({ time, title, detail }) => <button type="button" key={time}
						aria-pressed={seekTime === time} onClick={() => { setSeekTime(time); setPaused(true); }}>
						<strong>{title}</strong><span>{detail}</span>
					</button>)}
				</div>
				<p className={styles.reducedNote}>Redusert bevegelse · viser den ferdige Verevon-logoen.</p>
			</section>
		</main>
	);
}
