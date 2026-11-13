package main

import (
	"context"
	"encoding/base64"
	"testing"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/store"
)

// purgeCall is one retention-sweep purge that the recording store received.
type purgeCall struct {
	revokedBefore time.Time
	limit         int
}

// recordingZDRStore stands in for the acknowledgement ledger. Only the retention sweep's purge is
// implemented; the embedded interface is nil, so a call to any other method would panic.
type recordingZDRStore struct {
	store.ConnectionZDRStore
	purges chan purgeCall
}

func (s *recordingZDRStore) PurgeRevokedConnectionZDRAcknowledgements(_ context.Context, revokedBefore time.Time, limit int) (int, error) {
	select {
	case s.purges <- purgeCall{revokedBefore: revokedBefore, limit: limit}:
	default:
	}
	return 0, nil
}

// firstSweepPurge starts the sweep with cfg and reports the first purge it makes within window. A sweep
// that starts purges at once, so a false result means the sweep did not start.
func firstSweepPurge(t *testing.T, cfg config.Config, window time.Duration) (purgeCall, bool) {
	t.Helper()
	repo := &recordingZDRStore{purges: make(chan purgeCall, 8)}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	logger := zerolog.Nop()

	startZDRAcknowledgementSweep(ctx, cfg, repo, &logger)

	select {
	case call := <-repo.purges:
		return call, true
	case <-time.After(window):
		return purgeCall{}, false
	}
}

func TestZDRAcknowledgementSweepDoesNotStartWhileTheSwitchIsOff(t *testing.T) {
	cfg := config.Config{ZDRAcknowledgementEnabled: false, ZDRAcknowledgementRetentionDays: 30}
	if call, ran := firstSweepPurge(t, cfg, 200*time.Millisecond); ran {
		t.Fatalf("retention sweep ran with ZDR_ACKNOWLEDGEMENT_ENABLED off: revokedBefore=%s limit=%d", call.revokedBefore, call.limit)
	}
}

func TestZDRAcknowledgementSweepRunsAtStartWhileTheSwitchIsOn(t *testing.T) {
	cfg := config.Config{ZDRAcknowledgementEnabled: true, ZDRAcknowledgementRetentionDays: 30}
	call, ran := firstSweepPurge(t, cfg, 5*time.Second)
	if !ran {
		t.Fatal("retention sweep did not run at start with ZDR_ACKNOWLEDGEMENT_ENABLED on")
	}
	if call.limit != zdrAcknowledgementSweepBatch {
		t.Fatalf("limit = %d, want %d", call.limit, zdrAcknowledgementSweepBatch)
	}
	// The cutoff is the retention window counted back from the moment the sweep ran.
	want := time.Now().UTC().Add(-30 * 24 * time.Hour)
	if drift := want.Sub(call.revokedBefore); drift < -time.Minute || drift > time.Minute {
		t.Fatalf("revokedBefore = %s, want about %s (30 days back)", call.revokedBefore, want)
	}
}

// The sweep reads the same strict switch as the routes, through config.Load: only the literal "true"
// starts it, and the near misses ("1", "t", "TRUE") and an empty value leave it off.
func TestZDRAcknowledgementSweepFollowsTheStrictSwitchFromTheEnvironment(t *testing.T) {
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", base64.StdEncoding.EncodeToString([]byte("12345678901234567890123456789012")))
	for _, tc := range []struct {
		value string
		want  bool
	}{
		{value: "", want: false},
		{value: "1", want: false},
		{value: "t", want: false},
		{value: "TRUE", want: false},
		{value: "true", want: true},
	} {
		t.Run("value="+tc.value, func(t *testing.T) {
			t.Setenv("ZDR_ACKNOWLEDGEMENT_ENABLED", tc.value)
			cfg, err := config.Load()
			if err != nil {
				t.Fatalf("Load: %v", err)
			}
			window := 200 * time.Millisecond
			if tc.want {
				window = 5 * time.Second
			}
			if _, ran := firstSweepPurge(t, cfg, window); ran != tc.want {
				t.Fatalf("sweep started = %v with ZDR_ACKNOWLEDGEMENT_ENABLED=%q, want %v", ran, tc.value, tc.want)
			}
		})
	}
}
