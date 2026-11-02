"use client";

import { useState } from "react";
import { HomepageIntro } from "@/components/home/intro/HomepageIntro";
import styles from "./LogoLoadingPreview.module.css";

export function LogoLoadingPreview() {
	const [run, setRun] = useState(0);
	const [isPlaying, setIsPlaying] = useState(true);

	const replay = () => {
		setRun((current) => current + 1);
		setIsPlaying(true);
	};

	return (
		<main className={styles.preview} data-playing={isPlaying}>
			<header className={styles.header}>
				<div aria-label="Verevon" className={styles.brand} data-site-brand-target="">
					<span>VEREVON</span>
				</div>
			</header>

			<button className={styles.replay} onClick={replay} type="button">
				Spill på nytt
			</button>

			{isPlaying ? (
				<HomepageIntro key={run} onComplete={() => setIsPlaying(false)} playback="preview" />
			) : null}
		</main>
	);
}
