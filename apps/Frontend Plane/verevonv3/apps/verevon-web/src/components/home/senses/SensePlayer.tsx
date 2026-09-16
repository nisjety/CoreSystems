"use client";

import { Player, type PlayerRef } from "@remotion/player";
import { Pause, Play, RotateCcw } from "lucide-react";
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { SENSE_DURATION, SENSE_FPS, SENSE_STEPS, SenseScene, type SenseKind } from "./SenseScene";
import styles from "./SensesMotion.module.css";

const labels: Record<SenseKind, string[]> = {
	delegate: ["Deleger", "Koble sammen", "Få resultatet"],
	learn: ["Vis hvordan", "Verevon lærer", "Neste gang"],
	oversee: ["Følg arbeidet", "Vurder sammen", "Gi klarsignal"],
};

export default function SensePlayer({ kind, backgroundSrc }: { kind: SenseKind; backgroundSrc: string }) {
	const rootRef = useRef<HTMLDivElement>(null);
	const playerRef = useRef<PlayerRef>(null);
	const progressRef = useRef<HTMLDivElement>(null);
	const [inView, setInView] = useState(false);
	const [pageVisible, setPageVisible] = useState(true);
	const [paused, setPaused] = useState(false);
	const [reduced, setReduced] = useState(true);
	const [explicitPlay, setExplicitPlay] = useState(false);
	const [step, setStep] = useState(0);
	const playing = inView && pageVisible && !paused && (!reduced || explicitPlay);

	useEffect(() => {
		const query = window.matchMedia("(prefers-reduced-motion: reduce)");
		const update = () => {
			setReduced(query.matches);
			if (query.matches) { setExplicitPlay(false); playerRef.current?.seekTo(450); }
		};
		update();
		query.addEventListener("change", update);
		const onVisibility = () => setPageVisible(document.visibilityState === "visible");
		onVisibility();
		document.addEventListener("visibilitychange", onVisibility);
		const observer = new IntersectionObserver(([entry]) => setInView(entry.isIntersecting && entry.intersectionRatio >= .35), { threshold: [0, .35] });
		if (rootRef.current) observer.observe(rootRef.current);
		return () => {
			observer.disconnect();
			query.removeEventListener("change", update);
			document.removeEventListener("visibilitychange", onVisibility);
		};
	}, []);

	useEffect(() => {
		const player = playerRef.current;
		if (playing) player?.play();
		else player?.pause();
	}, [playing]);

	useEffect(() => {
		const player = playerRef.current;
		if (!player) return;
		let currentStep = -1;
		const onFrame = ({ detail }: { detail: { frame: number } }) => {
			if (progressRef.current) progressRef.current.style.transform = `scaleX(${detail.frame / (SENSE_DURATION - 1)})`;
			const nextStep = Math.min(2, Math.floor(detail.frame / 180));
			if (nextStep !== currentStep) { currentStep = nextStep; setStep(nextStep); }
		};
		player.addEventListener("frameupdate", onFrame);
		onFrame({ detail: { frame: player.getCurrentFrame() } });
		return () => player.removeEventListener("frameupdate", onFrame);
	}, []);

	function seek(index: number) {
		playerRef.current?.seekTo(paused || reduced ? [120, 285, 465][index] : SENSE_STEPS[index] + 28);
		setStep(index);
	}

	return <div className={styles.player} ref={rootRef}>
		<div aria-hidden="true" className={styles.canvas} style={{ "--sense-background": `url("${backgroundSrc}")` } as CSSProperties}>
			<div className={styles.scene}>
				<Player ref={playerRef} component={SenseScene} inputProps={{ kind }} durationInFrames={SENSE_DURATION} fps={SENSE_FPS} compositionWidth={720} compositionHeight={640} style={{ width: "100%", height: "100%", backgroundColor: "transparent" }} loop controls={false} clickToPlay={false} doubleClickToFullscreen={false} spaceKeyToPlayOrPause={false} initiallyMuted numberOfSharedAudioTags={0} />
			</div>
		</div>
		<div className={styles.transport}>
			<div className={styles.steps} aria-label="Steg i eksemplet">
				{labels[kind].map((label, i) => <button key={label} type="button" aria-pressed={step === i} onClick={() => seek(i)} className={styles.step}><span className={styles.stepTrack}><span style={{ transform: `scaleX(${step >= i ? 1 : 0})` }} /></span><span>{label}</span></button>)}
			</div>
			<button type="button" className={styles.control} aria-label={playing ? "Sett animasjonen på pause" : "Spill animasjonen"} onClick={() => { setPaused(playing); if (!playing) setExplicitPlay(true); }}>{playing ? <Pause size={17} /> : <Play size={17} />}</button>
			<button type="button" className={styles.control} aria-label="Spill eksemplet fra starten" onClick={() => { playerRef.current?.seekTo(0); setPaused(false); setExplicitPlay(true); }}><RotateCcw size={16} /></button>
		</div>
		<div aria-hidden="true" className={styles.progress}><div ref={progressRef} /></div>
	</div>;
}
