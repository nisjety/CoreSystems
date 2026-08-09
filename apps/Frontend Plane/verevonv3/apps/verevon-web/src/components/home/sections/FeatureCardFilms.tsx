"use client";

import Image from "next/image";
import {
	createContext,
	type ReactNode,
	useCallback,
	useContext,
	useEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import {
	FEATURE_FILM_DURATION_SECONDS,
	filmTimeDistance,
	needsFilmResync,
	normalizeFilmTime,
} from "./feature-film-playback";

export type FeatureFilmKind =
	| "build"
	| "connect"
	| "ground"
	| "approve"
	| "research"
	| "actions";

const FILMS: Record<
	FeatureFilmKind,
	{ mp4: string; poster: string; webm: string }
> = {
	build: {
		mp4: "/feature-films/build.mp4",
		poster: "/feature-films/build-poster.jpg",
		webm: "/feature-films/build.webm",
	},
	// "connect" originally had its own recording, but its background texture
	// (a purple/pink cell-like pattern) doesn't match the brand and the
	// mockup UI inside it still reads "VELION" from before the rename — both
	// need a re-record. Until then it reuses "approve" (not "ground" —
	// "ground" already repeats at "research" right next to this card, and
	// stacking a third identical card would be worse than the mismatch).
	connect: {
		mp4: "/feature-films/approve.mp4",
		poster: "/feature-films/approve-poster.jpg",
		webm: "/feature-films/approve.webm",
	},
	ground: {
		mp4: "/feature-films/ground.mp4",
		poster: "/feature-films/ground-poster.jpg",
		webm: "/feature-films/ground.webm",
	},
	approve: {
		mp4: "/feature-films/approve.mp4",
		poster: "/feature-films/approve-poster.jpg",
		webm: "/feature-films/approve.webm",
	},
	// Dedicated keys keep each card independently registered in the shared
	// film group. These are safe fallbacks until the two additional films are
	// rendered; the platform surfaces remain distinct and fully playable.
	research: {
		mp4: "/feature-films/ground.mp4",
		poster: "/feature-films/ground-poster.jpg",
		webm: "/feature-films/ground.webm",
	},
	actions: {
		mp4: "/feature-films/approve.mp4",
		poster: "/feature-films/approve-poster.jpg",
		webm: "/feature-films/approve.webm",
	},
};

type FilmRegistry = {
	register: (kind: FeatureFilmKind, video: HTMLVideoElement | null) => void;
};

const FilmRegistryContext = createContext<FilmRegistry | null>(null);

const wait = (milliseconds: number) =>
	new Promise<void>((resolve) => {
		window.setTimeout(resolve, milliseconds);
	});

const playWithTimeout = (video: HTMLVideoElement) =>
	Promise.race([video.play(), wait(650)]);

const waitForMetadata = (video: HTMLVideoElement) => {
	if (video.readyState >= HTMLMediaElement.HAVE_METADATA) {
		return Promise.resolve();
	}

	return new Promise<void>((resolve) => {
		let settled = false;
		const finish = () => {
			if (settled) return;
			settled = true;
			window.clearTimeout(timeout);
			video.removeEventListener("loadedmetadata", finish);
			video.removeEventListener("error", finish);
			resolve();
		};
		const timeout = window.setTimeout(finish, 1_500);

		video.addEventListener("loadedmetadata", finish, { once: true });
		video.addEventListener("error", finish, { once: true });
	});
};

const seekVideo = async (
	video: HTMLVideoElement,
	time: number,
) => {
	await waitForMetadata(video);

	const mediaDuration =
		Number.isFinite(video.duration) && video.duration > 0
			? video.duration
			: FEATURE_FILM_DURATION_SECONDS;
	const safeTime = Math.min(
		normalizeFilmTime(time, FEATURE_FILM_DURATION_SECONDS),
		Math.max(0, mediaDuration - 1 / 30),
	);

	if (
		filmTimeDistance(video.currentTime, safeTime) <=
		1 / 60
	) {
		return;
	}

	await new Promise<void>((resolve) => {
		let settled = false;
		const finish = () => {
			if (settled) return;
			settled = true;
			window.clearTimeout(timeout);
			video.removeEventListener("seeked", finish);
			video.removeEventListener("error", finish);
			resolve();
		};
		const timeout = window.setTimeout(finish, 750);

		video.addEventListener("seeked", finish, { once: true });
		video.addEventListener("error", finish, { once: true });

		try {
			video.currentTime = safeTime;
		} catch {
			finish();
		}
	});
};

const recordPlaybackResults = (
	videos: HTMLVideoElement[],
	results: PromiseSettledResult<void>[],
) => {
	results.forEach((result, index) => {
		if (result.status === "fulfilled") {
			delete videos[index].dataset.featureFilmError;
			return;
		}

		const reason =
			result.reason instanceof Error
				? result.reason.message
				: String(result.reason);
		videos[index].dataset.featureFilmError = reason;
		console.warn(`Verevon feature film could not start: ${reason}`);
	});
};

export function FeatureFilmGroup({
	children,
	controlClassName = "",
	playVisibleOnly = false,
	requireStageActivation = false,
}: {
	children: ReactNode;
	controlClassName?: string;
	playVisibleOnly?: boolean;
	requireStageActivation?: boolean;
}) {
	const anchorRef = useRef<HTMLDivElement>(null);
	const videosRef = useRef(
		new Map<FeatureFilmKind, HTMLVideoElement>(),
	);
	const sharedTimeRef = useRef(0);
	const [registryVersion, setRegistryVersion] = useState(0);
	const [registeredCount, setRegisteredCount] = useState(0);
	const [isAutoActive, setIsAutoActive] = useState(false);
	const [isCompactLayout, setIsCompactLayout] = useState(false);
	const [prefersReducedMotion, setPrefersReducedMotion] =
		useState(false);
	const [userPaused, setUserPaused] = useState(false);
	const [visibleKinds, setVisibleKinds] = useState(
		() => new Set<FeatureFilmKind>(),
	);

	const register = useCallback(
		(kind: FeatureFilmKind, video: HTMLVideoElement | null) => {
			const currentVideo = videosRef.current.get(kind);
			if (currentVideo === video || (!currentVideo && !video)) {
				return;
			}

			const nextVideos = new Map(videosRef.current);

			if (video) {
				nextVideos.set(kind, video);
			} else {
				nextVideos.delete(kind);
			}

			videosRef.current = nextVideos;
			setRegisteredCount(nextVideos.size);
			setRegistryVersion((version) => version + 1);
		},
		[],
	);

	const registry = useMemo(() => ({ register }), [register]);

	useEffect(() => {
		const anchor = anchorRef.current;
		if (!anchor) return undefined;

		const stage = anchor.closest<HTMLElement>(
			"[data-feature-output-stage]",
		);
		const intersectionTarget = stage ?? anchor.parentElement;
		if (!intersectionTarget) return undefined;

		const reducedMotion = window.matchMedia(
			"(prefers-reduced-motion: reduce)",
		);
		const compactLayout = window.matchMedia("(max-width: 1140px)");
		let inViewport = false;
		let stageActive =
			!requireStageActivation ||
			stage?.dataset.featureOutputActive === "true";

		const syncActiveState = () => {
			setPrefersReducedMotion(reducedMotion.matches);
			setIsCompactLayout(compactLayout.matches);
			setIsAutoActive(
				inViewport && stageActive && !reducedMotion.matches,
			);
		};
		syncActiveState();

		const intersectionObserver = new IntersectionObserver(
			([entry]) => {
				inViewport =
					entry.isIntersecting && entry.intersectionRatio >= 0.12;
				syncActiveState();
			},
			{ threshold: [0, 0.12] },
		);
		intersectionObserver.observe(intersectionTarget);

		const stageObserver =
			requireStageActivation && stage
				? new MutationObserver(() => {
						stageActive =
							stage.dataset.featureOutputActive === "true";
						syncActiveState();
					})
				: null;
		stageObserver?.observe(stage!, {
			attributeFilter: ["data-feature-output-active"],
			attributes: true,
		});

		const onMotionPreferenceChange = () => syncActiveState();
		reducedMotion.addEventListener(
			"change",
			onMotionPreferenceChange,
		);
		compactLayout.addEventListener(
			"change",
			onMotionPreferenceChange,
		);

		return () => {
			intersectionObserver.disconnect();
			stageObserver?.disconnect();
			reducedMotion.removeEventListener(
				"change",
				onMotionPreferenceChange,
			);
			compactLayout.removeEventListener(
				"change",
				onMotionPreferenceChange,
			);
		};
	}, [requireStageActivation]);

	useEffect(() => {
		const entries = Array.from(videosRef.current.entries());

		const shouldObserveVisibility = isCompactLayout || playVisibleOnly;

		if (!shouldObserveVisibility) {
			setVisibleKinds(new Set(entries.map(([kind]) => kind)));
			return undefined;
		}

		const kindByVideo = new Map(
			entries.map(([kind, video]) => [video, kind]),
		);
		const observer = new IntersectionObserver(
			(intersections) => {
				setVisibleKinds((currentKinds) => {
					const nextKinds = new Set(currentKinds);

					intersections.forEach((entry) => {
						const kind = kindByVideo.get(
							entry.target as HTMLVideoElement,
						);
						if (!kind) return;

						if (
							entry.isIntersecting &&
							entry.intersectionRatio >= 0.2
						) {
							nextKinds.add(kind);
						} else {
							nextKinds.delete(kind);
						}
					});

					if (
						nextKinds.size === currentKinds.size &&
						Array.from(nextKinds).every((kind) =>
							currentKinds.has(kind),
						)
					) {
						return currentKinds;
					}

					return nextKinds;
				});
			},
			{ threshold: [0, 0.2, 0.6] },
		);

		entries.forEach(([, video]) => observer.observe(video));

		return () => observer.disconnect();
	}, [isCompactLayout, playVisibleOnly, registryVersion]);

	useEffect(() => {
		const entries = Array.from(videosRef.current.entries());
		const allVideos = entries.map(([, video]) => video);
		const activeEntries = isCompactLayout || playVisibleOnly
			? entries.filter(([kind]) => visibleKinds.has(kind))
			: entries;
		const videos = activeEntries.map(([, video]) => video);
		const hasRequiredVideos = videos.length > 0;
		const playbackRequested =
			isAutoActive && !userPaused && hasRequiredVideos;

		if (!playbackRequested) {
			allVideos.forEach((video) => video.pause());
			if (!isAutoActive && !userPaused) {
				sharedTimeRef.current = 0;
			}
			return undefined;
		}

		let cancelled = false;
		const baseTime = sharedTimeRef.current;
		const wallClockStartedAt = performance.now();
		const getSharedTime = () =>
			normalizeFilmTime(
				baseTime +
					(performance.now() - wallClockStartedAt) / 1_000,
				FEATURE_FILM_DURATION_SECONDS,
			);
		const leader =
			activeEntries.find(([kind]) => kind === "build")?.[1] ??
			videos[0];

		allVideos
			.filter((video) => !videos.includes(video))
			.forEach((video) => video.pause());

		const startTogether = async () => {
			const targetTime = getSharedTime();
			await Promise.all(
				videos.map((video) => seekVideo(video, targetTime)),
			);

			if (cancelled) return;
			const playbackResults = await Promise.allSettled(
				videos.map(playWithTimeout),
			);
			recordPlaybackResults(videos, playbackResults);

			if (cancelled) return;
			const initialTimes = videos.map(
				(video) => video.currentTime,
			);
			await wait(650);

			if (cancelled) return;
			const webmStalled = videos.some(
				(video, index) =>
					video.currentSrc.endsWith(".webm") &&
					filmTimeDistance(
						initialTimes[index],
						video.currentTime,
					) < 0.12,
			);

			if (!webmStalled) return;

			videos.forEach((video) => {
				if (!video.currentSrc.endsWith(".webm")) return;
				video.pause();
				video.src = video.dataset.mp4Src ?? "";
				video.dataset.featureFilmFallback = "mp4";
				video.load();
			});

			const fallbackTarget = getSharedTime();
			await Promise.all(
				videos.map((video) =>
					seekVideo(video, fallbackTarget),
				),
			);

			if (cancelled) return;
			const fallbackResults = await Promise.allSettled(
				videos.map(playWithTimeout),
			);
			recordPlaybackResults(videos, fallbackResults);
		};

		void startTogether();

		const syncInterval = window.setInterval(() => {
			const leaderTime = normalizeFilmTime(
				leader.currentTime,
				FEATURE_FILM_DURATION_SECONDS,
			);

			videos.forEach((video) => {
				if (
					video !== leader &&
					needsFilmResync(
						leaderTime,
						video.currentTime,
						FEATURE_FILM_DURATION_SECONDS,
					)
				) {
					video.currentTime = leaderTime;
				}
			});
		}, 500);

		return () => {
			cancelled = true;
			sharedTimeRef.current = getSharedTime();
			window.clearInterval(syncInterval);
			allVideos.forEach((video) => video.pause());
		};
	}, [
		isAutoActive,
		isCompactLayout,
		playVisibleOnly,
		registryVersion,
		userPaused,
		visibleKinds,
	]);

	const playbackActive =
		isAutoActive && !userPaused && registeredCount > 0;

	return (
		<FilmRegistryContext.Provider value={registry}>
			<div
				className="contents"
				data-feature-film-active={
					playbackActive ? "true" : "false"
				}
				data-feature-film-group=""
				data-feature-film-paused={
					userPaused ? "true" : "false"
				}
				ref={anchorRef}
			>
				{children}
				{!prefersReducedMotion ? (
					<button
						aria-label={
							userPaused
								? "Start animasjonen"
								: "Stopp animasjonen"
						}
						aria-pressed={userPaused}
						className={`absolute z-50 inline-flex size-9 items-center justify-center rounded-full border border-verevon-j-text/12 bg-background/82 text-verevon-j-text/64 shadow-[0_8px_24px_rgba(23,23,23,0.06)] backdrop-blur-md transition hover:border-verevon-j-text/24 hover:text-verevon-j-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-verevon-coral motion-reduce:hidden ${controlClassName}`}
						onClick={() =>
							setUserPaused((isPaused) => !isPaused)
						}
						title={
							userPaused
								? "Start animasjonen"
								: "Stopp animasjonen"
						}
						type="button"
					>
						{userPaused ? (
							<span
								aria-hidden="true"
								className="ml-0.5 block size-0 border-y-[5px] border-l-[8px] border-y-transparent border-l-current"
							/>
						) : (
							<span
								aria-hidden="true"
								className="flex gap-[3px]"
							>
								<span className="h-[10px] w-[2px] bg-current" />
								<span className="h-[10px] w-[2px] bg-current" />
							</span>
						)}
					</button>
				) : null}
			</div>
		</FilmRegistryContext.Provider>
	);
}

export function FeatureCardFilm({
	kind,
}: {
	kind: FeatureFilmKind;
}) {
	const registry = useContext(FilmRegistryContext);
	const film = FILMS[kind];
	const setVideoRef = useCallback(
		(video: HTMLVideoElement | null) => {
			registry?.register(kind, video);
		},
		[kind, registry],
	);

	return (
		<>
			<Image
				alt=""
				aria-hidden="true"
				className="hidden object-cover motion-reduce:z-10 motion-reduce:block"
				fill
				sizes="(max-width: 1140px) 92vw, 22vw"
				src={film.poster}
			/>
			<video
				aria-hidden="true"
				className="absolute inset-0 size-full object-cover motion-reduce:hidden"
				disablePictureInPicture
				loop
				muted
				playsInline
				poster={film.poster}
				preload="metadata"
				ref={setVideoRef}
				tabIndex={-1}
				data-mp4-src={film.mp4}
			>
				<source src={film.webm} type="video/webm" />
				<source src={film.mp4} type="video/mp4" />
			</video>
		</>
	);
}
