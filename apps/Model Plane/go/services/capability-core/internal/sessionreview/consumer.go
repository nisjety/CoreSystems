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
	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
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
// Every run that is not reviewed is logged as "learning review skipped" with
// a reason, apart from "learning review completed", which only a review that
// reached the model logs, with its candidate/selected/persisted counts.
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
	outcome, err := handleRunCompleted(ctx, data, sc, ic, model)
	return outcome.Persisted, err
}

// Skip reasons decided here rather than by the transcript source.
const (
	skipNotRunCompleted  learning.SkipReason = "not_run_completed"
	skipRetentionPosture learning.SkipReason = "retention_posture"
	skipProviderRefusal  learning.SkipReason = "provider_refusal"
)

// handleRunCompleted is [HandleRunCompleted] returning the full outcome.
func handleRunCompleted(
	ctx context.Context,
	data []byte,
	sc reviewSessionClient,
	ic reviewInferenceClient,
	model string,
) (learning.ReviewOutcome, error) {
	var env envelope.Envelope
	if err := json.Unmarshal(data, &env); err != nil {
		return learning.ReviewOutcome{}, fmt.Errorf("sessionreview: decode envelope: %w", err)
	}
	ref, ok := ParseRunCompleted(&env)
	if !ok {
		// Not a run-completion we act on. Not logged: every other lifecycle
		// event on the stream lands here.
		return learning.ReviewOutcome{Skipped: skipNotRunCompleted}, nil
	}
	// Retention gate — ahead of every session-core read, by construction.
	if posture := parseRetentionPosture(data); !posture.AllowsDerivedPersistence() {
		slog.Info("learning review skipped", "run_id", ref.RunID,
			"reason", skipRetentionPosture, "posture", posture.String())
		return learning.ReviewOutcome{Skipped: skipRetentionPosture}, nil
	}
	reviewer := llmreviewer.NewReviewer(ic, model, ref.OrgID)
	sink := skillsink.NewSessionCoreSink(sc, ref.OrgID)
	src := NewSessionCoreTranscriptSource(sc)
	outcome, err := OnRunCompleted(ctx, ref, src, reviewer, sink)
	var responseError *llmreviewer.ResponseError
	if errors.As(err, &responseError) && responseError.Kind == "refused" {
		slog.Info("learning review skipped", "run_id", ref.RunID, "reason", skipProviderRefusal)
		return learning.ReviewOutcome{Skipped: skipProviderRefusal}, nil
	}
	switch {
	case err != nil:
		// Logged, with its delivery count, by whoever settles the message.
	case outcome.Skipped != "":
		slog.Info("learning review skipped", "run_id", ref.RunID, "reason", outcome.Skipped)
	default:
		slog.Info("learning review completed", "run_id", ref.RunID,
			"candidates", outcome.Candidates, "selected", outcome.Selected,
			"skills_persisted", outcome.Persisted)
	}
	return outcome, err
}

// Delay transient/schema failures rather than redelivering at once; the
// delivery count they are bounded by is the consumer's MaxDeliver.
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

// Fallbacks for when the bound consumer's own limits cannot be read. They
// equal what nats-provisioner provisions for SkillReviewDurable.
const (
	defaultReviewAckWait    = 120 * time.Second
	defaultReviewMaxDeliver = 5
)

// reviewAckMargin is how long before the consumer's AckWait a review is cut
// off, leaving time to settle the message before JetStream redelivers it.
const reviewAckMargin = 20 * time.Second

// reviewLimits are the delivery limits of the bound durable. RunConsumer reads
// them from the server at bind time, so nats-provisioner is their one source
// and this consumer cannot drift from it.
type reviewLimits struct {
	ackWait    time.Duration
	maxDeliver int // <= 0: unlimited
}

func defaultReviewLimits() reviewLimits {
	return reviewLimits{ackWait: defaultReviewAckWait, maxDeliver: defaultReviewMaxDeliver}
}

// limitsFromConsumer takes the limits from the consumer's server-side config,
// keeping the default for an ack wait the server did not report.
func limitsFromConsumer(info *nats.ConsumerInfo) reviewLimits {
	limits := defaultReviewLimits()
	if info == nil {
		return limits
	}
	if info.Config.AckWait > 0 {
		limits.ackWait = info.Config.AckWait
	}
	limits.maxDeliver = info.Config.MaxDeliver
	return limits
}

// deadline bounds one review below the ack wait (100 s for the provisioned
// 120 s). A review still running when the ack wait expires is redelivered and
// reviewed a second time alongside the first.
func (l reviewLimits) deadline() time.Duration {
	if d := l.ackWait - reviewAckMargin; d >= l.ackWait/2 {
		return d
	}
	return l.ackWait / 2
}

// exhausted reports whether this delivery was the consumer's last.
func (l reviewLimits) exhausted(delivery uint64) bool {
	return l.maxDeliver > 0 && delivery >= uint64(l.maxDeliver)
}

// reviewDelivery is the part of a JetStream message a review settles.
// *nats.Msg satisfies it; tests inject a recorder.
type reviewDelivery interface {
	Metadata() (*nats.MsgMetadata, error)
	Ack(opts ...nats.AckOpt) error
	NakWithDelay(delay time.Duration, opts ...nats.AckOpt) error
	Term(opts ...nats.AckOpt) error
	InProgress(opts ...nats.AckOpt) error
}

// settleReview runs one review under the per-message deadline and settles its
// delivery: Ack when it completed or was skipped; Term on a permanent
// (configuration/authorization) failure; otherwise, including a review that
// ran out of time, a delayed Nak until the consumer's MaxDeliver is reached.
func settleReview(
	ctx context.Context,
	msg reviewDelivery,
	subject string,
	limits reviewLimits,
	review func(context.Context) (learning.ReviewOutcome, error),
) {
	// The ack wait runs from delivery, and a pushed message can sit in the
	// client queue behind another review for most of it. Restarting the ack
	// wait here is what makes the deadline below fall inside it.
	if err := msg.InProgress(); err != nil {
		slog.Warn("sessionreview: in-progress ack failed", "subject", subject, "error", err)
	}
	reviewCtx, cancel := context.WithTimeout(ctx, limits.deadline())
	outcome, herr := review(reviewCtx)
	timedOut := errors.Is(reviewCtx.Err(), context.DeadlineExceeded)
	cancel()
	if herr != nil {
		metadata, metadataErr := msg.Metadata()
		var delivery, sequence uint64
		if metadataErr == nil {
			delivery = metadata.NumDelivered
			sequence = metadata.Sequence.Stream
		}
		if !timedOut && permanentReviewFailure(herr) {
			slog.Error("learning review permanently failed; retained event requires recovery", "stream", RunEventsStream, "stream_sequence", sequence, "subject", subject, "code", status.Code(herr).String())
			if err := msg.Term(); err != nil {
				slog.Warn("sessionreview: term failed", "error", err)
			}
			return
		}
		if limits.exhausted(delivery) {
			// Leave the original envelope in the stream, unacknowledged. Operators
			// can inspect/replay this exact sequence after fixing the cause, without
			// logging customer content or silently marking a failed review done.
			slog.Error("learning review exhausted; retained event requires recovery", "stream", RunEventsStream, "stream_sequence", sequence, "subject", subject, "delivery", delivery, "max_deliver", limits.maxDeliver, "timed_out", timedOut, "error", herr)
			return
		}
		delay := reviewRetryDelay(delivery)
		slog.Warn("learning review failed; delayed redelivery", "subject", subject, "stream_sequence", sequence, "delivery", delivery, "retry_after", delay, "timed_out", timedOut, "deadline", limits.deadline(), "error", herr)
		if nerr := msg.NakWithDelay(delay); nerr != nil {
			slog.Warn("sessionreview: nak failed", "subject", subject, "error", nerr)
		}
		return
	}
	if outcome.Persisted > 0 {
		slog.Info("learning review persisted skills", "count", outcome.Persisted)
	}
	if aerr := msg.Ack(); aerr != nil {
		slog.Warn("sessionreview: ack failed", "subject", subject, "error", aerr)
	}
}

// RunConsumer binds the durable JetStream consumer (SkillReviewDurable on
// RunEventsStream) with manual ack and runs a learning review for each
// RUN_COMPLETED. Best-effort and self-contained: per-message decode/skip
// outcomes are always acked; a genuine review failure is Nak'd so JetStream
// redelivers it (bounded by the consumer's MaxDeliver, set server-side by
// nats-provisioner). Each review is cut off below the consumer's AckWait (see
// [reviewLimits.deadline]); both limits are read from the consumer at bind
// time. Blocks until ctx is cancelled.
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
	// Read before binding so the very first delivery is already settled
	// against the server's limits. The bind below performs this same lookup,
	// so it needs no permission the consumer did not already have.
	limits := defaultReviewLimits()
	if info, err := js.ConsumerInfo(RunEventsStream, SkillReviewDurable); err != nil {
		slog.Warn("learning-review consumer limits unreadable; using provisioned defaults",
			"stream", RunEventsStream, "durable", SkillReviewDurable, "error", err,
			"ack_wait", limits.ackWait, "max_deliver", limits.maxDeliver)
	} else {
		limits = limitsFromConsumer(info)
	}
	sub, err := js.QueueSubscribe(RunCompletedSubject, SkillReviewDurable, func(msg *nats.Msg) {
		settleReview(ctx, msg, msg.Subject, limits, func(reviewCtx context.Context) (learning.ReviewOutcome, error) {
			return handleRunCompleted(reviewCtx, msg.Data, sc, ic, model)
		})
	}, nats.Bind(RunEventsStream, SkillReviewDurable), nats.ManualAck())
	if err != nil {
		return fmt.Errorf("sessionreview: bind %s/%s: %w", RunEventsStream, SkillReviewDurable, err)
	}
	slog.Info("learning-review consumer bound", "stream", RunEventsStream, "durable", SkillReviewDurable, "subject", RunCompletedSubject,
		"ack_wait", limits.ackWait, "max_deliver", limits.maxDeliver, "review_deadline", limits.deadline())
	<-ctx.Done()
	_ = sub.Unsubscribe()
	return nil
}
