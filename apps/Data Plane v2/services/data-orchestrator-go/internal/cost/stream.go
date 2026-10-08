package cost

import (
	"errors"
	"fmt"
	"slices"
	"time"

	"github.com/nats-io/nats.go"
)

// The durable stream the usage ledger rides (design task CR-T10).
//
// The cost events were a plain core-NATS publish read by a plain core subscription: a
// consumer that was restarting, or one that crashed between receiving an event and
// writing the row, lost it, and no producer could know. Now producers publish to a
// JetStream stream and wait for the broker's acknowledgement, and this service reads it
// through a durable consumer that acknowledges after the row is written.
//
// The stream binds the subject the cost events always used. A stream captures a core
// publish on its subject, so a producer that has not been upgraded keeps working and its
// events are retained too, and a JetStream publish is also delivered to core subscribers,
// so a consumer that has not been upgraded still sees the events. That is the "dual
// publish for one release" with no second publish call (which would have booked every
// event twice). A subject belongs to exactly one stream: infra/nats/SUBJECTS.md.
//
// The Rust side defines the same stream (services/usage-ledger-rs/src/stream.rs) with the
// same values, because the producers create it too (a producer may start before this
// service does). scripts/check-subjects.sh fails when the two names differ.
const (
	// StreamName is the stream's name. Mirrored in usage-ledger-rs (STREAM_NAME).
	StreamName = "DATAPLANE_COST"

	// DurableName is this service's consumer. Replicas share it as a queue group.
	DurableName = "data-orchestrator-cost"

	// streamMaxAge is how long an event is kept: a consumer that is down over a weekend
	// loses nothing. It is also the horizon an expired envelope is still accepted within
	// (maxBacklogAge): an envelope's own lifetime is two minutes, so everything that waits
	// in the stream longer than that is expired when it is read.
	streamMaxAge = 7 * 24 * time.Hour

	// streamMaxBytes bounds the store. Limits retention never discards on
	// acknowledgement, so a consumer that is down for a long time would otherwise fill the
	// broker.
	streamMaxBytes = 512 * 1024 * 1024

	// streamDuplicateWindow is the window inside which the broker drops a second publish of
	// the same Nats-Msg-Id, which is what makes a producer's publish retry harmless.
	streamDuplicateWindow = 2 * time.Minute
)

// StreamConfig is the stream as it is defined.
//
// Retention is Limits and must stay Limits. Interest and WorkQueue both discard a message
// that has no consumer bound when it arrives, and the producer-before-consumer order of a
// deploy would lose every event in between while the stream looked healthy.
func StreamConfig() *nats.StreamConfig {
	return &nats.StreamConfig{
		Name:       StreamName,
		Subjects:   []string{SubjectCostLedger},
		Retention:  nats.LimitsPolicy,
		Storage:    nats.FileStorage,
		MaxAge:     streamMaxAge,
		MaxBytes:   streamMaxBytes,
		Discard:    nats.DiscardOld,
		Duplicates: streamDuplicateWindow,
		Replicas:   1,
	}
}

var errStreamUnusable = errors.New("usage ledger stream is unusable")

// convergeStream says what an existing stream needs to be the ledger's: an error when it
// cannot be (a setting that cannot be changed in place is wrong), the configuration to
// update to when something that can be changed is behind, nil when it is right. It only
// ever raises a limit: an operator who widened one on a live broker keeps it.
func convergeStream(current nats.StreamConfig) (*nats.StreamConfig, error) {
	if current.Retention != nats.LimitsPolicy {
		return nil, fmt.Errorf("%w: %s has %s retention, the ledger needs limits retention (an event published while no consumer is bound would be discarded); retention cannot be changed in place, delete and recreate the stream",
			errStreamUnusable, StreamName, current.Retention)
	}
	if current.Storage != nats.FileStorage {
		return nil, fmt.Errorf("%w: %s has %s storage, the ledger must survive a broker restart",
			errStreamUnusable, StreamName, current.Storage)
	}
	upgraded := current
	changed := false
	if !slices.Contains(upgraded.Subjects, SubjectCostLedger) {
		upgraded.Subjects = append(slices.Clone(upgraded.Subjects), SubjectCostLedger)
		changed = true
	}
	if upgraded.MaxAge != 0 && upgraded.MaxAge < streamMaxAge {
		upgraded.MaxAge = streamMaxAge
		changed = true
	}
	if upgraded.MaxBytes > 0 && upgraded.MaxBytes < streamMaxBytes {
		upgraded.MaxBytes = streamMaxBytes
		changed = true
	}
	if upgraded.Duplicates < streamDuplicateWindow {
		upgraded.Duplicates = streamDuplicateWindow
		changed = true
	}
	if !changed {
		return nil, nil
	}
	return &upgraded, nil
}

// EnsureStream creates the stream, or checks and converges the one that exists. It is
// idempotent: every producer and this consumer call it on boot.
func EnsureStream(js nats.JetStreamManager) error {
	info, err := js.StreamInfo(StreamName)
	switch {
	case err == nil:
		upgraded, err := convergeStream(info.Config)
		if err != nil {
			return err
		}
		if upgraded != nil {
			if _, err := js.UpdateStream(upgraded); err != nil {
				return fmt.Errorf("converge usage ledger stream %s: %w", StreamName, err)
			}
		}
		return nil
	case errors.Is(err, nats.ErrStreamNotFound):
		if _, err := js.AddStream(StreamConfig()); err != nil {
			// The commonest cause is the subject being bound to another stream already (a
			// subject belongs to exactly one): say so.
			return fmt.Errorf("create usage ledger stream %s on %s (is the subject bound to another stream?): %w",
				StreamName, SubjectCostLedger, err)
		}
		return nil
	default:
		return fmt.Errorf("read usage ledger stream %s: %w", StreamName, err)
	}
}
