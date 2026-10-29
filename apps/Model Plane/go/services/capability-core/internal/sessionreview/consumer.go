package sessionreview

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"

	"github.com/nats-io/nats.go"
	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/services/capability-core/internal/llmreviewer"
	"github.com/triodelab/model-plane/services/capability-core/internal/skillsink"
	"google.golang.org/grpc"
)

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
	return OnRunCompleted(ctx, ref, src, reviewer, sink)
}

// RunConsumer subscribes to run-lifecycle events and runs a learning review for
// each RUN_COMPLETED. Best-effort and self-contained: per-message errors are
// logged, never fatal; it returns when ctx is cancelled.
//
// The subscription path is exercised against a real NATS server by
// scripts/tests/learning-nats-trigger-test.sh. The injected gRPC clients still
// need a deployed session-core/inference-core/LLM for a full customer proof;
// the decode→review→persist logic is unit-tested with fakes here.
func RunConsumer(
	ctx context.Context,
	nc *nats.Conn,
	sc reviewSessionClient,
	ic reviewInferenceClient,
	model string,
) error {
	sub, err := nc.Subscribe(RunCompletedSubject, func(msg *nats.Msg) {
		n, herr := HandleRunCompleted(ctx, msg.Data, sc, ic, model)
		switch {
		case herr != nil:
			slog.Warn("learning review failed", "subject", msg.Subject, "error", herr)
		case n > 0:
			slog.Info("learning review persisted skills", "count", n)
		}
	})
	if err != nil {
		return fmt.Errorf("sessionreview: subscribe %s: %w", RunCompletedSubject, err)
	}
	slog.Info("learning-review consumer started", "subject", RunCompletedSubject)
	<-ctx.Done()
	_ = sub.Unsubscribe()
	return nil
}
