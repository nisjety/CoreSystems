package provisioner

import (
	"context"
	"errors"
	"fmt"

	"github.com/nats-io/nats.go"
)

// These old names are recognized only to preserve existing broker history.
const legacyControlSharedStream = "AQENCIA_CONTROLPLANE"
const archivedControlSubject = "_ARCHIVE.CONTROLPLANE.LEGACY.>"

type NamespaceSnapshot struct {
	Stream    *nats.StreamInfo     `json:"stream"`
	Consumers []*nats.ConsumerInfo `json:"consumers"`
}

func namespaceIsArchived(info *nats.StreamInfo) bool {
	return len(info.Config.Subjects) == 1 && info.Config.Subjects[0] == archivedControlSubject
}

func requireMigratedSharedNamespace(js nats.JetStreamContext) error {
	info, err := js.StreamInfo(legacyControlSharedStream)
	if errors.Is(err, nats.ErrStreamNotFound) {
		return nil
	}
	if err != nil {
		return fmt.Errorf("inspect prior Control namespace: %w", err)
	}
	if !namespaceIsArchived(info) {
		return errors.New("prior Control stream is still active; run nats-namespace-migrate with publishers paused and consumers drained before provisioning CONTROLPLANE")
	}
	return nil
}

// MigrateControlSharedNamespace changes capture ownership, never message history
// or durable positions. It requires a quiescent bus and a saved pre-change snapshot.
// A failed provisioning attempt is resumable: keep publishers paused and rerun.
func MigrateControlSharedNamespace(ctx context.Context, js nats.JetStreamContext, apply, publishersPaused bool, save func(NamespaceSnapshot) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if apply && !publishersPaused {
		return errors.New("--publishers-paused is required when applying the namespace migration")
	}
	info, err := js.StreamInfo(legacyControlSharedStream)
	if errors.Is(err, nats.ErrStreamNotFound) {
		if apply {
			return ProvisionControlSharedRuntime(ctx, js)
		}
		return nil
	}
	if err != nil {
		return fmt.Errorf("inspect prior stream: %w", err)
	}
	if namespaceIsArchived(info) {
		if apply {
			return ProvisionControlSharedRuntime(ctx, js)
		}
		return nil
	}
	if _, err := js.StreamInfo(ControlSharedStreamName); err == nil {
		return errors.New("both prior and target streams exist; refusing an ambiguous migration")
	} else if !errors.Is(err, nats.ErrStreamNotFound) {
		return err
	}
	snapshot := NamespaceSnapshot{Stream: info, Consumers: []*nats.ConsumerInfo{}}
	for consumer := range js.ConsumersInfo(legacyControlSharedStream, nats.Context(ctx)) {
		snapshot.Consumers = append(snapshot.Consumers, consumer)
		if consumer.NumPending != 0 || consumer.NumAckPending != 0 {
			return fmt.Errorf("consumer %s is not drained: %d pending, %d ack-pending", consumer.Name, consumer.NumPending, consumer.NumAckPending)
		}
	}
	if info.State.Msgs > 0 && len(snapshot.Consumers) == 0 {
		return errors.New("prior stream has messages but no consumers; delivery completion cannot be established")
	}
	if len(snapshot.Consumers) != info.State.Consumers {
		return errors.New("consumer inventory changed or failed; refusing migration")
	}
	if !apply {
		return nil
	}
	if save == nil {
		return errors.New("a private snapshot must be saved before migration")
	}
	if err := save(snapshot); err != nil {
		return fmt.Errorf("save namespace snapshot: %w", err)
	}
	current, err := js.StreamInfo(legacyControlSharedStream)
	if err != nil {
		return err
	}
	if current.State.LastSeq != info.State.LastSeq || current.State.Consumers != info.State.Consumers {
		return errors.New("stream changed during preflight; stop publishers and retry")
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	config := current.Config
	config.Subjects = []string{archivedControlSubject}
	if _, err := js.UpdateStream(&config); err != nil {
		return fmt.Errorf("archive prior capture subjects: %w", err)
	}
	if err := ProvisionControlSharedRuntime(ctx, js); err != nil {
		return fmt.Errorf("prior history and consumers are preserved; keep publishers paused and retry target provisioning: %w", err)
	}
	return nil
}
