package api

import (
	"context"
	"encoding/json"
	"log/slog"
	"net/http"
	"strings"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
	"github.com/triodelab/model-plane/services/capability-core/internal/sessionreview"
	"google.golang.org/grpc"
)

// ---------------------------------------------------------------------------
// LearnCommandHandler  POST /api/v1/learning/learn (EP-11 S7)
// ---------------------------------------------------------------------------

const learnCommandEnv = "LEARN_COMMAND_ENABLED"

// LearnCommandAPIFromLookup reads LEARN_COMMAND_ENABLED strictly: only the literal "true" turns
// /learn on, so a typo or a stray "1" leaves the route unregistered.
func LearnCommandAPIFromLookup(lookup func(string) string) bool {
	return lookup(learnCommandEnv) == "true"
}

// learnZDRFeature is the zdr-refusals.json feature a /learn refusal names.
const learnZDRFeature = "learn_command"

// learnZDRMessage is the catalogue sentence of zdr_feature_not_supported with that feature's
// label, as mp-contracts' Refusal::message writes it.
const learnZDRMessage = "This is not supported with zero data retention yet: learning from this conversation. The request was refused before any model call."

const (
	// maxLearnBodyBytes bounds the body before it is read: a thread id and a 500-character note.
	maxLearnBodyBytes = 8 << 10
	// maxLearnThreadIDBytes matches the bound the SPA's action contract puts on a thread id.
	maxLearnThreadIDBytes = 128
	// learnPrepareTimeout bounds the gates and the transcript read the person waits for.
	learnPrepareTimeout = 10 * time.Second
	// learnRunTimeout bounds the model call and the writes after the 202, as the run
	// consumer's review deadline does.
	learnRunTimeout = 100 * time.Second
)

// learnThreadOwners is the one RunService question /learn asks: does the caller own the thread.
// The generated mpv1.RunServiceClient satisfies it.
type learnThreadOwners interface {
	ResolveThreadOwner(ctx context.Context, in *mpv1.ResolveThreadOwnerRequest, opts ...grpc.CallOption) (*mpv1.ResolveThreadOwnerResponse, error)
}

// learnPreparer is the /learn pipeline: *sessionreview.Learner satisfies it.
type learnPreparer interface {
	Prepare(ctx context.Context, request sessionreview.LearnRequest) (learnRunner, learning.SkipReason, error)
}

// learnRunner is a /learn that cleared every gate.
type learnRunner interface {
	Run(ctx context.Context) (learning.ReviewOutcome, error)
}

// sessionreviewLearner adapts *sessionreview.Learner to learnPreparer, whose job type is an
// interface so a test can stand in for the model call.
type sessionreviewLearner struct {
	learner *sessionreview.Learner
}

func (s sessionreviewLearner) Prepare(ctx context.Context, request sessionreview.LearnRequest) (learnRunner, learning.SkipReason, error) {
	job, skip, err := s.learner.Prepare(ctx, request)
	if job == nil {
		return nil, skip, err
	}
	return job, skip, err
}

// LearnCommandHandler serves /learn: the signed-in person asks Verevon to distil one of their own
// threads into a skill proposal. It runs the learning review's pipeline with every gate the
// background review applies, answers each refusal at once, and answers 202 once the transcript is
// read and counted; the model call and the proposal write run after the answer. Nothing is
// enabled by it: a proposal waits in the person's learning inbox for their decision.
type LearnCommandHandler struct {
	enabled bool
	owners  learnThreadOwners
	learner learnPreparer
	// spawn runs a cleared /learn after the 202. A test runs it inline.
	spawn func(func())
}

// NewLearnCommandHandler builds the handler. With the switch off Register mounts nothing, so the
// route answers the mux's own 404, exactly as before it existed. A nil client or learner leaves
// the switched-on route answering 503.
func NewLearnCommandHandler(enabled bool, owners mpv1.RunServiceClient, learner *sessionreview.Learner) *LearnCommandHandler {
	h := &LearnCommandHandler{enabled: enabled, spawn: func(run func()) { go run() }}
	if owners != nil {
		h.owners = owners
	}
	if learner != nil {
		h.learner = sessionreviewLearner{learner: learner}
	}
	return h
}

// Register mounts POST /api/v1/learning/learn while the switch is on.
func (h *LearnCommandHandler) Register(mux *http.ServeMux) {
	if !h.enabled {
		return
	}
	mux.HandleFunc("/api/v1/learning/learn", h.learn)
}

type learnBody struct {
	ThreadID string `json:"thread_id"`
	Note     string `json:"note"`
}

func (h *LearnCommandHandler) learn(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeTypedError(w, http.StatusMethodNotAllowed, "method_not_allowed", "use POST")
		return
	}
	principal, ok := authctx.PrincipalFromContext(r.Context())
	if !ok || principal.OrganizationID == "" || principal.ActorID == "" {
		writeTypedError(w, http.StatusUnauthorized, "unauthenticated", "authentication required")
		return
	}
	// A learned skill is a person's own: a service or an API key cannot ask for one.
	if principal.PrincipalType != "user" {
		writeTypedError(w, http.StatusForbidden, "learn_requires_person", "only a signed-in person can ask Verevon to learn from a conversation")
		return
	}
	// authz refuses a ZDR principal before this handler; the check stays here so the refusal is
	// the typed one and cannot depend on the middleware's order. Nothing has been read yet.
	if principal.ZeroDataRetention || !principal.RetentionPolicyPresent {
		writeLearnZDRRefusal(w)
		return
	}
	r.Body = http.MaxBytesReader(w, r.Body, maxLearnBodyBytes)
	decoder := json.NewDecoder(r.Body)
	decoder.DisallowUnknownFields()
	var body learnBody
	if err := decoder.Decode(&body); err != nil {
		writeTypedError(w, http.StatusBadRequest, "invalid_body", "the body must be {thread_id, note?}")
		return
	}
	threadID := strings.TrimSpace(body.ThreadID)
	if threadID == "" || len(threadID) > maxLearnThreadIDBytes {
		writeTypedError(w, http.StatusBadRequest, "invalid_thread_id", "thread_id is required")
		return
	}
	note := strings.TrimSpace(body.Note)
	if len([]rune(note)) > learning.MaxLearnNoteChars {
		writeTypedError(w, http.StatusUnprocessableEntity, "learn_note_too_long", "the note is longer than 500 characters")
		return
	}
	if h.owners == nil || h.learner == nil {
		writeTypedError(w, http.StatusServiceUnavailable, "learn_unavailable", "learning from a conversation is unavailable")
		return
	}

	ctx, cancel := context.WithTimeout(r.Context(), learnPrepareTimeout)
	defer cancel()
	// Ownership first: the review reads the thread as a service, which is org-scoped only, so
	// the caller must be shown to own it before any gate reads anything of it.
	owned, err := h.owners.ResolveThreadOwner(ctx, &mpv1.ResolveThreadOwnerRequest{
		ThreadId: threadID, OrgId: principal.OrganizationID, UserId: principal.ActorID,
	})
	if err != nil {
		slog.Warn("learn: thread owner lookup failed", "error", err)
		writeTypedError(w, http.StatusServiceUnavailable, "learn_unavailable", "learning from a conversation is unavailable")
		return
	}
	if !owned.GetAuthorized() {
		writeTypedError(w, http.StatusNotFound, "learn_thread_not_found", "no such conversation")
		return
	}
	job, skip, err := h.learner.Prepare(ctx, sessionreview.LearnRequest{
		OrgID: principal.OrganizationID, UserID: principal.ActorID, ThreadID: threadID, Note: note,
	})
	if err != nil {
		slog.Warn("learn: gates or transcript read failed", "thread_id", threadID, "error", err)
		writeTypedError(w, http.StatusServiceUnavailable, "learn_unavailable", "learning from a conversation is unavailable")
		return
	}
	if skip != "" {
		slog.Info("learn refused", "thread_id", threadID, "reason", skip)
		writeLearnSkip(w, skip)
		return
	}
	// The review outlives the request: the person has their answer, and the proposal reaches
	// their inbox when the model is done. The request's values stay, its cancellation does not.
	runCtx := context.WithoutCancel(r.Context())
	h.spawn(func() {
		ctx, cancel := context.WithTimeout(runCtx, learnRunTimeout)
		defer cancel()
		if _, err := job.Run(ctx); err != nil {
			// Logged without content: the error names positions and fixed kinds only.
			slog.Warn("learn review failed", "thread_id", threadID, "error", err)
		}
	})
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusAccepted)
	_ = json.NewEncoder(w).Encode(map[string]any{"data": map[string]any{"state": "queued"}})
}

// writeLearnZDRRefusal answers the catalogue's zdr_feature_not_supported for learn_command, in
// the body mp-contracts writes, with the feature also under details as the envelope rule asks.
func writeLearnZDRRefusal(w http.ResponseWriter) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusPreconditionFailed)
	_ = json.NewEncoder(w).Encode(map[string]any{"error": map[string]any{
		"code":      "zdr_feature_not_supported",
		"message":   learnZDRMessage,
		"retryable": false,
		"action":    "turn_off_feature",
		"feature":   learnZDRFeature,
		"details":   map[string]any{"feature": learnZDRFeature},
	}})
}

// writeLearnSkip answers the skip reason a /learn was refused with. A reason this build does not
// name is answered as unavailable, never as queued.
func writeLearnSkip(w http.ResponseWriter, skip learning.SkipReason) {
	switch skip {
	case "retention_posture":
		// Zero data retention is the catalogue's refusal, never a /learn code of its own.
		writeLearnZDRRefusal(w)
	case "memory_state":
		writeTypedError(w, http.StatusConflict, "learn_memory_paused", "memory is paused or off, so nothing is learned from your conversations")
	case "org_policy":
		writeTypedError(w, http.StatusForbidden, "learn_org_review_disabled", "your organization has not turned on learning from conversations")
	case "owner_unknown":
		writeTypedError(w, http.StatusUnauthorized, "unauthenticated", "authentication required")
	case "thread_unknown":
		writeTypedError(w, http.StatusBadRequest, "invalid_thread_id", "thread_id is required")
	case learning.SkipConversationScope:
		writeTypedError(w, http.StatusUnprocessableEntity, "learn_thread_conversation_scope", "this conversation is kept out of learning")
	case learning.SkipEmptyTranscript:
		writeTypedError(w, http.StatusUnprocessableEntity, "learn_thread_empty", "this conversation has nothing to learn from yet")
	case "transcript_too_large":
		writeTypedError(w, http.StatusUnprocessableEntity, "learn_thread_too_large", "this conversation is too long to learn from")
	case "user_daily_cap":
		writeTypedError(w, http.StatusTooManyRequests, "learn_daily_cap", "you have asked Verevon to learn as often as today allows")
	case "org_daily_cap":
		writeTypedError(w, http.StatusTooManyRequests, "learn_daily_cap", "your organization has reached today's learning limit")
	case "budget_unavailable":
		writeTypedError(w, http.StatusServiceUnavailable, "learn_budget_unavailable", "learning cannot be counted right now, so it does not run")
	default:
		writeTypedError(w, http.StatusServiceUnavailable, "learn_unavailable", "learning from a conversation is unavailable")
	}
}
