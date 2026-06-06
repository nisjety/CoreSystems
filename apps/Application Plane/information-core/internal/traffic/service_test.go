package traffic

import "testing"

func TestStableTrafficMetrics(t *testing.T) {
	volume, speed := stableTrafficMetrics("abc")
	if volume < 550 || volume > 1499 {
		t.Fatalf("volume out of range: %d", volume)
	}
	if speed < 55 || speed > 89 {
		t.Fatalf("speed out of range: %d", speed)
	}
}

func TestDistanceKm(t *testing.T) {
	got := round2(distanceKm(59.9139, 10.7522, 59.9127, 10.7461))
	if got <= 0 {
		t.Fatalf("distanceKm() = %v", got)
	}
}
