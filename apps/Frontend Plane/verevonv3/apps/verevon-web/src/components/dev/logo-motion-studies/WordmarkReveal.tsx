import type { ReactNode } from "react";
import { WORDMARK } from "./wordmark-motion";
import styles from "./LogoMotionStudies.module.css";

export function WordmarkReveal({ children }: { children: ReactNode }) {
	return <div className={styles.lockupViewport} data-lockup="" role="img" aria-label="Verevon — merket og navnet">
		<div className={styles.lockupMark} data-lockup-mark="">{children}</div>
		<div className={styles.wordmark} aria-hidden="true">
			{Array.from(WORDMARK, (letter, index) => <span className={styles.letterWindow} key={index}>
				<span data-wordmark-letter={index} id={`wordmark-letter-${index}`}>{letter}</span>
			</span>)}
		</div>
	</div>;
}
