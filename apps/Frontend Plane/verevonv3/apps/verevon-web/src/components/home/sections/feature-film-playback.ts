export const FEATURE_FILM_DURATION_SECONDS = 12;
export const MAX_FILM_DRIFT_SECONDS = 0.08;

export function filmTimeDistance(
	firstTime: number,
	secondTime: number,
	duration = FEATURE_FILM_DURATION_SECONDS,
) {
	const first = normalizeFilmTime(firstTime, duration);
	const second = normalizeFilmTime(secondTime, duration);
	const directDistance = Math.abs(first - second);

	return Math.min(directDistance, duration - directDistance);
}

export function needsFilmResync(
	leaderTime: number,
	followerTime: number,
	duration = FEATURE_FILM_DURATION_SECONDS,
) {
	return (
		filmTimeDistance(leaderTime, followerTime, duration) -
			MAX_FILM_DRIFT_SECONDS >
		0.000_001
	);
}

export function normalizeFilmTime(time: number, duration: number) {
	if (!Number.isFinite(duration) || duration <= 0) {
		return 0;
	}

	return ((time % duration) + duration) % duration;
}
