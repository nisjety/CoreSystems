import { describe, expect, it } from "vitest";
import {
	FEATURE_FILM_DURATION_SECONDS,
	filmTimeDistance,
	MAX_FILM_DRIFT_SECONDS,
	needsFilmResync,
	normalizeFilmTime,
} from "./feature-film-playback";

describe("synchronized feature-film playback", () => {
	it("does not seek for imperceptible decoder drift", () => {
		expect(needsFilmResync(4, 4 + MAX_FILM_DRIFT_SECONDS)).toBe(false);
	});

	it("resynchronizes when a card drifts beyond the tolerance", () => {
		expect(needsFilmResync(4, 4.2)).toBe(true);
	});

	it("treats opposite sides of the loop boundary as adjacent", () => {
		expect(
			filmTimeDistance(
				0.02,
				FEATURE_FILM_DURATION_SECONDS - 0.02,
			),
		).toBeCloseTo(0.04);
		expect(
			needsFilmResync(
				0.02,
				FEATURE_FILM_DURATION_SECONDS - 0.02,
			),
		).toBe(false);
	});

	it("resynchronizes real drift close to the loop boundary", () => {
		expect(
			needsFilmResync(
				0.2,
				FEATURE_FILM_DURATION_SECONDS - 0.2,
			),
		).toBe(true);
	});

	it("normalizes the shared playhead at the loop boundary", () => {
		expect(normalizeFilmTime(12, 12)).toBe(0);
		expect(normalizeFilmTime(12.4, 12)).toBeCloseTo(0.4);
	});
});
