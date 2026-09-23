package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"os"
	"time"

	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/services/capability-core/internal/sessionreview"
)

// Recovery is deliberately one original envelope, supplied by an operator
// with permission to inspect the logged JetStream sequence. It neither resets
// a durable consumer nor broadens this service's NATS permissions.
func replayLearningEvent(ctx context.Context, path string) error {
	f, err := os.Open(path)
	if err != nil {
		return fmt.Errorf("cannot open retained event file")
	}
	defer f.Close()
	data, err := io.ReadAll(io.LimitReader(f, 1_048_577))
	if err != nil || len(data) > 1_048_576 {
		return fmt.Errorf("event unreadable or larger than 1 MiB")
	}
	var event envelope.Envelope
	if json.Unmarshal(data, &event) != nil {
		return fmt.Errorf("invalid retained event JSON")
	}
	ref, ok := sessionreview.ParseRunCompleted(&event)
	if !ok {
		return fmt.Errorf("event is not a scoped RUN_COMPLETED envelope")
	}
	sc, ic, _ := dialBackends()
	if sc == nil || ic == nil {
		return fmt.Errorf("review backends are not configured")
	}
	ctx, cancel := context.WithTimeout(ctx, 2*time.Minute)
	defer cancel()
	n, err := sessionreview.HandleRunCompleted(ctx, data, sc, ic, os.Getenv("LEARNING_REVIEW_MODEL"))
	if err != nil {
		return err
	}
	slog.Info("learning recovery completed", "run_id", ref.RunID, "skills_persisted", n)
	return nil
}
