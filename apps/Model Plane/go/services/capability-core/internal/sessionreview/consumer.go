package sessionreview

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"time"

	"github.com/nats-io/nats.go"
	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/services/capability-core/internal/llmreviewer"
	"github.com/triodelab/model-plane/services/capability-core/internal/skillsink"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// RunEventsStream is the JetStream stream RunConsumer's durable is bound
// to. Must match nats-provisioner's stream name exactly (services/
// nats-provisioner/main.go's streamConfigs, "MODEL_PLANE_RUN_EVENTS") --
// the same stream internal/runwatch.Notifier binds a separate durable to.
const RunEventsStream = "MODEL_PLANE_RUN_EVENTS"

// SkillReviewDurable is this consumer's durable name. Must exactly match
// the Durable value nats-provisioner registers server-side for this stream
// (see its consumerBindings) -- binding is Bind-only (never
// self-provisioning), so a name mismatch here fails RunConsumer's subscribe
// outright rather than silently missing events, the same contract
// runwatch.RunWatchDurable already relies on.
const SkillReviewDurable = "capability-core-skill-review"

// RunCompletedSubject is the NATS subject filter for run lifecycle events;
// RUN_COMPLETED envelopes arrive here and ParseRunCompleted selects them.
const RunCompletedSubject = "mp.v1.run.*.event"

// reviewSessionClient is the session-core surface the learning review needs:
// the transcript reader (ListConversation + ListAgentSkills, via the embedded
// sessionCoreReader) plus the skill writer (UpsertAgentSkill, for the sink).
// The generated *mpv1.SessionCoreClient satisfies it; tests inject a fake.
type reviewSessionClient interface {
	sessionCoreReader
	UpsertAgentSkill(ctx context.Context, in *mpv1.UpsertAgentSkillRequest, opts ...grpc.CallOption) (*mpv1.UpsertAgentSkillResponse, error)
}

// reviewInferenceClient is the inference-core surface the reviewer needs.
type reviewInferenceClient interface {
	Infer(ctx context.Context, in *mpv1.InferRequest, opts ...grpc.CallOption) (*mpv1.InferResponse, error)
}

// HandleRunCompleted decodes a run-lifecycle envelope and, if it is a
// RUN_COMPLETED, runs one learning review end-to-end: fetch the transcript +
// existing skills (session-core), review (inference-core), and persist the
// survivors (session-core, provenance-guarded). Returns the number persisted
// (0 for any non-trigger event). This is the WHOLE trigger path from bytes to
// persistence and is unit-tested with fake clients.
//
// # Zero Data Retention gate
//
// A review turns conversation content into a durable skill, so this function is
// a content-persisting boundary and must not act on a no-retention run. The
// posture is checked BEFORE the transcript source is built, so a ZDR (or
// unattested) run never causes even a READ of the conversation — reading it and
// discarding the result later would already have moved the content across the
// boundary. Anything other than an explicit `zdr: false` is skipped; see
// [RetentionPosture] for why absence fails closed and what that costs.
func HandleRunCompleted(
	ctx context.Context,
	data []byte,
	sc reviewSessionClient,
	ic reviewInferenceClient,
	model string,
) (int, error) {
	var env envelope.Envelope
	if err := json.Unmarshal(data, &env); err != nil {
		return 0, fmt.Errorf("sessionreview: decode envelope: %w", err)
	}
	ref, ok := ParseRunCompleted(&env)
	if !ok {
		return 0, nil // not a run-completion we act on
	}
	// Retention gate — ahead of every session-core read, by construction.
	if posture := parseRetentionPosture(data); !posture.AllowsDerivedPersistence() {
		slog.Info("learning review skipped: run retention posture forbids derived persistence",
			"run_id", ref.RunID, "posture", posture.String())
		return 0, nil
	}
	reviewer := llmreviewer.NewReviewer(ic, model, ref.OrgID)
	sink := skillsink.NewSessionCoreSink(sc, ref.OrgID)
	src := NewSessionCoreTranscriptSource(sc)
	n, err := OnRunCompleted(ctx, ref, src, reviewer, sink)
	var responseError *llmreviewer.ResponseError
	if errors.As(err, &responseError) && responseError.Kind == "refused" {
		slog.Info("learning review skipped: provider refusal", "run_id", ref.RunID)
		return 0, nil
	}
	if err == nil {
		slog.Info("learning review completed", "run_id", ref.RunID, "skills_persisted", n)
	}
	return n, err
}

// Match the provisioned five-delivery limit. Delay transient/schema failures
// instead of immediately replaying the same cached provider response five times.
func reviewRetryDelay(delivery uint64) time.Duration {
	delays := []time.Duration{30 * time.Second, 2 * time.Minute, 10 * time.Minute, 30 * time.Minute}
	if delivery < 1 {
		delivery = 1
	}
	if delivery > uint64(len(delays)) {
		return delays[len(delays)-1]
	}
	return delays[delivery-1]
}

func permanentReviewFailure(err error) bool {
	switch status.Code(err) {
	case codes.InvalidArgument, codes.PermissionDenied, codes.Unauthenticated, codes.FailedPrecondition:
		return true
	default:
		return false
	}
}

// RunConsumer binds the durable JetStream consumer (SkillReviewDurable on
// RunEventsStream) with manual ack and runs a learning review for each
// RUN_COMPLETED. Best-effort and self-contained: per-message decode/skip
// outcomes are always acked; a genuine review failure is Nak'd so JetStream
// redelivers it (bounded by the consumer's MaxDeliver, set server-side by
// nats-provisioner). Blocks until ctx is cancelled.
//
// # Why JetStream, not a plain subscribe
//
// This used to be a plain core-NATS nc.Subscribe -- fire-and-forget, at-most-
// once, with no persistence and no redelivery. That was recorded elsewhere
// (internal/runwatch's package doc) as an acceptable tradeoff because this
// consumer's side effects were characterized as "cheap local reads/writes" --
// which is not what HandleRunCompleted actually does: it reads a full
// conversation transcript from session-core, calls inference-core for a real
// LLM review, and writes a durable skill back to session-core. Losing a
// message silently here means a run's transcript is never reviewed and never
// contributes a skill -- exactly the failure this project verified live on
// 2026-09-17: session-core had durably published 88 RUN_COMPLETED learning
// events (every one JetStream-acked, none stuck) while capability-core's
// plain subscribe received zero of them, and agent_skills held zero rows
// despite 147 completed runs. A durable JetStream consumer, mirroring
// internal/runwatch.Notifier's already-proven pattern, closes that gap: a
// message published while this process is down or reconnecting is still
// waiting in the stream the next time it binds.
//
// js must already have SkillReviewDurable provisioned on RunEventsStream (see
// nats-provisioner) -- this call binds, it never self-creates the consumer.
//
// The subscription path is exercised against a real NATS+JetStream server by
// TestRunConsumerAgainstLiveNATS. The injected gRPC clients still need a
// deployed session-core/inference-core/LLM for a full customer proof; the
// decode→review→persist logic is unit-tested with fakes here.
func RunConsumer(
	ctx context.Context,
	js nats.JetStreamContext,
	sc reviewSessionClient,
	ic reviewInferenceClient,
	model string,
) error {
	sub, err := js.QueueSubscribe(RunCompletedSubject, SkillReviewDurable, func(msg *nats.Msg) {
		n, herr := HandleRunCompleted(ctx, msg.Data, sc, ic, model)
		switch {
		case herr != nil:
			metadata, metadataErr := msg.Metadata()
			var delivery, sequence uint64
			if metadataErr == nil {
				delivery = metadata.NumDelivered
				sequence = metadata.Sequence.Stream
			}
			if permanentReviewFailure(herr) {
				slog.Error("learning review permanently failed; retained event requires recovery", "stream", RunEventsStream, "stream_sequence", sequence, "subject", msg.Subject, "code", status.Code(herr).String())
				if err := msg.Term(); err != nil {
					slog.Warn("sessionreview: term failed", "error", err)
				}
				return
			}
			if delivery >= 5 {
				// Leave the original envelope in the stream, unacknowledged. Operators
				// can inspect/replay this exact sequence after fixing the cause, without
				// logging customer content or silently marking a failed review done.
				slog.Error("learning review exhausted; retained event requires recovery", "stream", RunEventsStream, "stream_sequence", sequence, "subject", msg.Subject, "delivery", delivery, "error", herr)
				return
			}
			delay := reviewRetryDelay(delivery)
			slog.Warn("learning review failed; delayed redelivery", "subject", msg.Subject, "stream_sequence", sequence, "delivery", delivery, "retry_after", delay, "error", herr)
			if nerr := msg.NakWithDelay(delay); nerr != nil {
				slog.Warn("sessionreview: nak failed", "subject", msg.Subject, "error", nerr)
			}
			return
		case n > 0:
			slog.Info("learning review persisted skills", "count", n)
		}
		if aerr := msg.Ack(); aerr != nil {
			slog.Warn("sessionreview: ack failed", "subject", msg.Subject, "error", aerr)
		}
	}, nats.Bind(RunEventsStream, SkillReviewDurable), nats.ManualAck())
	if err != nil {
		return fmt.Errorf("sessionreview: bind %s/%s: %w", RunEventsStream, SkillReviewDurable, err)
	}
	slog.Info("learning-review consumer bound", "stream", RunEventsStream, "durable", SkillReviewDurable, "subject", RunCompletedSubject)
	<-ctx.Done()
	_ = sub.Unsubscribe()
	return nil
}
