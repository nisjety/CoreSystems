"use client";

import { Player, type PlayerRef } from "@remotion/player";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { Pause, Play } from "lucide-react";
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { SENSE_FPS, SenseScene, type SenseKind } from "./SenseScene";
import { formatSenseTime, getSensePlayback, seekFrameFromSeconds, shouldPlaySense } from "./sense-player-playback";
import styles from "./SensesMotion.module.css";

export default function SensePlayer({ kind, backgroundSrc }: { kind: SenseKind; backgroundSrc: string }) {
	const playback = getSensePlayback(kind);
	const rootRef = useRef<HTMLDivElement>(null);
	const playerRef = useRef<PlayerRef>(null);
	const [inView, setInView] = useState(false);
	const [pageVisible, setPageVisible] = useState(true);
	const [paused, setPaused] = useState(false);
	const [reduced, setReduced] = useState(true);
	const [explicitPlay, setExplicitPlay] = useState(false);
	const [completed, setCompleted] = useState(false);
	const [currentFrame, setCurrentFrame] = useState(0);
	const playing = shouldPlaySense({ inView, pageVisible, paused, reduced, explicitPlay, completed });

	useEffect(() => {
		const query = window.matchMedia("(prefers-reduced-motion: reduce)");
		const update = () => {
			const prefersReducedMotion = query.matches && document.documentElement.dataset.motion !== "full";
			setReduced(prefersReducedMotion);
			if (prefersReducedMotion) {
				setExplicitPlay(false);
				playerRef.current?.seekTo(playback.loop ? 450 : playback.durationInFrames - 1);
				if (!playback.loop) setCompleted(true);
			}
		};
		update();
		query.addEventListener("change", update);
		const onVisibility = () => setPageVisible(document.visibilityState === "visible");
		onVisibility();
		document.addEventListener("visibilitychange", onVisibility);
		const trigger = rootRef.current ? ScrollTrigger.create({
			trigger: rootRef.current,
			start: "top bottom",
			end: "bottom top",
			onToggle: (self) => setInView(self.isActive),
		}) : null;
		setInView(trigger?.isActive ?? false);
		return () => {
			trigger?.kill();
			query.removeEventListener("change", update);
			document.removeEventListener("visibilitychange", onVisibility);
		};
	}, [playback.durationInFrames, playback.loop]);

	useEffect(() => {
		const player = playerRef.current;
		if (playing) player?.play();
		else player?.pause();
	}, [playing]);

	useEffect(() => {
		const player = playerRef.current;
		if (!player) return;
		const onFrame = ({ detail }: { detail: { frame: number } }) => {
			setCurrentFrame(detail.frame);
		};
		if (playback.showControls) player.addEventListener("frameupdate", onFrame);
		const onEnded = () => { if (!playback.loop) setCompleted(true); };
		player.addEventListener("ended", onEnded);
		if (playback.showControls) onFrame({ detail: { frame: player.getCurrentFrame() } });
		return () => {
			if (playback.showControls) player.removeEventListener("frameupdate", onFrame);
			player.removeEventListener("ended", onEnded);
		};
	}, [playback.loop, playback.showControls]);

	function seek(seconds: number) {
		const frame = seekFrameFromSeconds(seconds, SENSE_FPS, playback.durationInFrames);
		playerRef.current?.seekTo(frame);
		setCurrentFrame(frame);
	}

	function togglePlayback() {
		setPaused(playing);
		if (!playing) setExplicitPlay(true);
	}

	return <div className={styles.player} data-sense-in-view={inView} data-sense-playing={playing} data-sense-completed={completed} ref={rootRef}>
		<div aria-hidden="true" className={styles.canvas} style={{ "--sense-background": `url("${backgroundSrc}")` } as CSSProperties}>
			<div className={styles.scene}>
				<Player ref={playerRef} component={SenseScene} inputProps={{ kind }} durationInFrames={playback.durationInFrames} fps={SENSE_FPS} compositionWidth={720} compositionHeight={640} style={{ width: "100%", height: "100%", backgroundColor: "transparent" }} loop={playback.loop} moveToBeginningWhenEnded={false} controls={false} clickToPlay={false} doubleClickToFullscreen={false} spaceKeyToPlayOrPause={false} initiallyMuted numberOfSharedAudioTags={0} />
			</div>
		</div>
		{playback.showControls ? <div className={styles.controls} data-sense-controls>
			<span className={styles.time} data-sense-time>{formatSenseTime(currentFrame / SENSE_FPS)} / {formatSenseTime(playback.durationInFrames / SENSE_FPS)}</span>
			<input aria-label="Spol i animasjonen" aria-valuetext={`${formatSenseTime(currentFrame / SENSE_FPS)} av ${formatSenseTime(playback.durationInFrames / SENSE_FPS)}`} className={styles.timeline} min={0} max={playback.durationInFrames / SENSE_FPS} step={1 / SENSE_FPS} type="range" value={currentFrame / SENSE_FPS} onInput={(event) => seek(Number(event.currentTarget.value))} style={{ background: `linear-gradient(to right, #f47753 ${(currentFrame / (playback.durationInFrames - 1)) * 100}%, #d6d6d3 0%)` }} />
			<button type="button" className={styles.control} aria-label={playing ? "Sett animasjonen på pause" : paused ? "Fortsett animasjonen" : "Spill animasjonen"} aria-pressed={playing} onClick={togglePlayback}>{playing ? <Pause aria-hidden="true" size={16} /> : <Play aria-hidden="true" size={16} fill="currentColor" />}</button>
		</div> : null}
	</div>;
}
