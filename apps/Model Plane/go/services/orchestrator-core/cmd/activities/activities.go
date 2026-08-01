// Package activities defines Temporal activity implementations for orchestrator-core.
package activities

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"sort"
	"strings"
	"time"

	"github.com/google/uuid"
	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/pkg/natsx"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/feedback"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/grpcclient"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/servicecred"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// ── Type definitions ─────────────────────────────────────────────────────────

type RunMetadata struct {
	RunID     string
	ThreadID  string
	OrgID     string
	UserID    string
	StartedAt time.Time
}
type StepResult struct {
	StepIndex     int
	ToolName      string
	Output        string
	NeedsApproval bool
	Completed     bool
	Metadata      map[string]string
}
type StepLoopInput struct {
	RunID    string
	ThreadID string
	Goal     string
	Policy   string
	MaxTurns int
	// OrgID is the tenant scope for the step's tools.
	OrgID string
	// UserID is the acting viewer. Threaded to ExecuteStep.user_id so
	// viewer-scoped tools (knowledge-search) filter to the caller's visible set
	// rather than the whole org. Empty = org-scoped (legacy).
	UserID string
}
type StepLoopOutput struct {
	Steps     []StepResult
	Completed bool
	Summary   string
}

// StepInput is one durable turn of the step loop for ExecuteStepActivity. The
// workflow-owned per-turn driver supplies StepIndex deterministically
// (0..MaxTurns-1); it is echoed onto the returned StepResult so the turn's
// ordinal matches what the legacy in-activity loop assigned. OrgID (tenant) and
// UserID (viewer) carry the same scoping semantics as StepLoopInput.
type StepInput struct {
	RunID     string
	OrgID     string
	UserID    string
	StepIndex int
}

// Retention is a run's Zero Data Retention posture as it travels from the
// workflow input to the lifecycle envelope. It is a tri-state on purpose: a
// bool would force a workflow that was never told a posture to assert one, and
// asserting "retainable" is the failure mode that leaks content.
//
// The zero value is [RetentionUnspecified], so a construction site that forgets
// to supply a posture fails closed instead of silently claiming durability.
type Retention string

const (
	// RetentionUnspecified means no posture was attested. The envelope carries
	// no `zdr` key and every downstream content-persisting consumer must refuse
	// (capability-core's learning review already does).
	RetentionUnspecified Retention = ""
	// RetentionZeroData means the run is Zero Data Retention: its lifecycle
	// envelope is suppressed before any NATS backend.
	RetentionZeroData Retention = "zero_data_retention"
	// RetentionDurable means an issuer explicitly attested that this run's
	// content may be retained. Only this value admits run-derived content into
	// an event payload.
	RetentionDurable Retention = "durable"
)

// RetentionFor maps an ATTESTED boolean posture onto a Retention. Only call it
// where the caller's retention claim is known to have been present; when the
// claim is absent use [RetentionUnspecified] instead of RetentionFor(false),
// which would upgrade "nobody said" into "explicitly retainable".
func RetentionFor(zdr bool) Retention {
	if zdr {
		return RetentionZeroData
	}
	return RetentionDurable
}

// AllowsContent reports whether run-derived content (a summary, a model
// critique) may be placed in an event payload under this posture.
func (r Retention) AllowsContent() bool { return r == RetentionDurable }

// envelopeFlag renders the posture as the envelope's tri-state `zdr` field.
func (r Retention) envelopeFlag() *bool {
	switch r {
	case RetentionZeroData:
		return envelope.ZDRFlag(true)
	case RetentionDurable:
		return envelope.ZDRFlag(false)
	default:
		return nil
	}
}

// CompletionInput is the terminal input for a successful run.
//
// Retention carries the run's ZDR posture so CompleteRunActivity can stamp the
// lifecycle envelope. Without it the RUN_COMPLETED envelope declares nothing,
// and capability-core's skill-learning review — which admits only an explicit
// `zdr: false` — skips every run, leaving the learning loop inert.
type CompletionInput struct {
	RunID, OrgID, UserID, Summary string
	Retention                     Retention
}

// FailureInput is the compensation input for a failed run. Retention has the
// same meaning as on [CompletionInput]: the failure reason can embed downstream
// error text, so a Zero Data Retention run's RUN_FAILED envelope must be
// suppressed exactly like its RUN_COMPLETED one.
type FailureInput struct {
	RunID, OrgID, UserID, Reason string
	Retention                    Retention
}
type MemoryQueryInput struct {
	OrgID    string
	Since    time.Time
	MaxItems int
}
type MemoryEntry struct {
	ID        string
	OrgID     string
	Content   string
	ThreadID  string
	CreatedAt time.Time
}
type ConsolidationInput struct{ Entries []MemoryEntry }
type ConsolidationOutput struct {
	ConsolidatedEntries []MemoryEntry
	Summary             string
}
type WriteMemoryInput struct{ Entries []MemoryEntry }
type SkillValidationInput struct{ SkillID, OrgID string }
type SkillValidationOutput struct {
	Valid  bool
	Errors []string
}
type PromotionGateInput struct{ SkillID, FromScope, ToScope, OrgID string }
type PromotionGateOutput struct {
	Passed bool
	Checks []string
}
type RegistryUpdateInput struct{ SkillID, FromScope, NewScope, OrgID string }

// ── Struct + constructor ─────────────────────────────────────────────────────

type Activities struct {
	logger        *slog.Logger
	clients       *grpcclient.Clients
	publisher     *natsx.Publisher
	feedbackStore feedback.Store
}

func NewActivities(logger *slog.Logger, clients *grpcclient.Clients) *Activities {
	return &Activities{logger: logger, clients: clients}
}

// SetPublisher wires a NATS publisher for emitting run lifecycle envelopes.
func (a *Activities) SetPublisher(p *natsx.Publisher) { a.publisher = p }

// SetFeedbackStore wires the durable operator-rating store that backs the
// feedback → skill-promotion loop.
func (a *Activities) SetFeedbackStore(f feedback.Store) { a.feedbackStore = f }

// RecordFeedback persists one operator rating. Called by the
// `mp.v1.feedback.rated` subscriber, not by Temporal — the rating must land
// durably whether or not any workflow is running.
func (a *Activities) RecordFeedback(ctx context.Context, r feedback.Rating) error {
	if a.feedbackStore == nil {
		return errors.New("feedback store not configured")
	}
	return a.feedbackStore.Record(ctx, r)
}

// SystemActorID is the reserved actor stamped on a lifecycle envelope for a run
// that has no human behind it — a cron-fired task, a maintenance sweep.
//
// The value is orchestrator-core's own service-principal subject because that is
// what session-core persists into `runs.user_id` for a run this service owns (see
// its `SYSTEM_RUN_OWNERS`). Any other spelling — a `system:` sentinel, say —
// would put one actor string on the row and a different one on the run's events,
// leaving nothing able to join the two.
//
// It remains a pre-run FALLBACK only: once StartRun answers, the owner it echoes
// is authoritative and is what gets stamped.
//
// envelope.Validate() requires user_id, and Encode is never reached when it
// fails, so before this constant existed EVERY system-initiated run silently
// dropped its RUN_COMPLETED / RUN_FAILED envelope: the publish failed
// validation and the only trace was a log line. capability-core's learning
// consumer, which subscribes to exactly those events, could therefore never see
// one. The colon makes the value unmistakably non-human: Auth Core actor ids are
// base62, so this can never collide with a real user.
const SystemActorID = "service:orchestrator-core"

// publishRunEvent emits one run-lifecycle envelope.
//
// retention is stamped onto the envelope's `zdr` field. A [RetentionZeroData]
// event is dropped by natsx.Publisher before it reaches any backend, so nothing
// is emitted at all; a [RetentionUnspecified] event is emitted with no `zdr`
// key, which downstream content-persisting consumers must treat as a refusal.
// Callers are responsible for keeping run-derived content out of payload unless
// retention.AllowsContent() — suppression protects the ZDR case, but an
// unattested run is published and mp.v1.run.*.event is JetStream-retained.
func (a *Activities) publishRunEvent(runID, orgID, userID, eventType, idemSuffix string, retention Retention, payload any) {
	if a.publisher == nil {
		return
	}
	data, err := json.Marshal(payload)
	if err != nil {
		a.logger.Error("marshal run event payload", "err", err, "event", eventType)
		return
	}
	// System-initiated runs have no acting viewer. Stamp the reserved actor
	// rather than let envelope validation reject — and therefore discard — the
	// run's whole lifecycle event.
	if strings.TrimSpace(userID) == "" {
		userID = SystemActorID
	}
	env := &envelope.Envelope{
		EventID:        uuid.NewString(),
		EventType:      eventType,
		SchemaVersion:  1,
		Ts:             time.Now().UTC(),
		Producer:       "orchestrator-core",
		OrgID:          orgID,
		UserID:         userID,
		CorrelationID:  runID,
		ResourceRef:    "run/" + runID,
		IdempotencyKey: runID + ":" + idemSuffix,
		Payload:        data,
		Zdr:            retention.envelopeFlag(),
	}
	if err := env.Validate(); err != nil {
		a.logger.Error("envelope validate", "err", err, "event", eventType)
		return
	}
	encoded, err := env.Encode()
	if err != nil {
		a.logger.Error("envelope encode", "err", err, "event", eventType)
		return
	}
	if err := a.publisher.Publish(natsx.RunEventSubject(runID), encoded); err != nil {
		a.logger.Error("nats publish", "err", err, "event", eventType, "run_id", runID)
	}
}

// ── Activity 1: StartRunActivity ─────────────────────────────────────────────

// userBoundCredentialRequired is session-core's refusal when the caller proved a
// valid service identity but no human: `start_run` ends with
// `caller.user_id().ok_or_else(|| permission_denied("user-bound run credential
// required"))`, because a run row is owned by a user.
//
// It is matched by message because the gRPC code alone (PermissionDenied) is also
// returned for a tenant mismatch and for thread-ownership failures, which are
// real authorization errors an operator must not confuse with this one.
const userBoundCredentialRequired = "user-bound run credential required"

func (a *Activities) StartRunActivity(ctx context.Context, runID, threadID, orgID, userID string) (RunMetadata, error) {
	fallback := RunMetadata{RunID: runID, ThreadID: threadID, OrgID: orgID, UserID: userID, StartedAt: time.Now().UTC()}
	if a.clients == nil || a.clients.SessionCore == nil {
		a.logger.Warn("downstream unavailable, skipping", "method", "StartRun")
		return fallback, nil
	}
	client := mpv1.NewSessionCoreClient(a.clients.SessionCore)

	// A run needs a thread, and a system run needs one THIS service owns:
	// session-core requires the thread's owner to equal the run's owner exactly on
	// that path, so that an allowlisted workload cannot park a system run inside a
	// person's thread. The thread id the dispatcher supplies defaults to the run
	// id and names nothing that exists, so provision one here.
	//
	// Keyed on the run id so the create is idempotent: session-core returns the
	// existing thread for a repeated (org, owner, session_key), which matters
	// because this activity is retried and a fresh id per attempt would leak a
	// thread per retry.
	if strings.TrimSpace(userID) == "" {
		created, terr := client.CreateThread(ctx, &mpv1.CreateThreadRequest{
			SessionKey: "system-run/" + runID,
			OrgId:      orgID,
		})
		switch {
		case terr != nil && status.Code(terr) == codes.Unavailable:
			a.logger.Warn("SessionCore unavailable", "method", "CreateThread")
			return fallback, nil
		case terr != nil:
			return fallback, fmt.Errorf("provision system thread for %s: %w", runID, terr)
		default:
			threadID = created.ThreadId
			fallback.ThreadID = threadID
		}
	}

	resp, err := client.StartRun(ctx, &mpv1.StartRunRequest{
		ThreadId: threadID,
		OrgId:    orgID,
		UserId:   userID,
	})
	if err != nil {
		if status.Code(err) == codes.Unavailable {
			a.logger.Warn("SessionCore unavailable", "method", "StartRun")
			return fallback, nil
		}
		// Name the cause. This activity has no inbound credential to forward —
		// its context comes from the Temporal worker — so it presents
		// orchestrator-core's own minted service token, and a service token's
		// subject is the service (auth-core's issueInternalToken sets
		// `userId: principal.subject` and has no delegation field). session-core
		// authenticates it, accepts the `session:write` scope, matches the org
		// and clears thread ownership, then refuses at the last gate because a
		// run must be owned by a person.
		//
		// Retrying cannot help and the bare status hides why, so the error says
		// what has to change instead: either the run is created by the request
		// that has the user's credential and this workflow only supervises it
		// (which is what StartWorkflowRequest.run_id already describes), or
		// session-core gains an explicit owner for system-initiated runs — the
		// reserved SystemActorID above is the convention that would express it.
		if status.Code(err) == codes.PermissionDenied &&
			strings.Contains(err.Error(), userBoundCredentialRequired) {
			a.logger.Error(
				"StartRun refused: a service credential cannot create a user-owned run",
				"run_id", runID, "org_id", orgID, "has_user", userID != "",
				"remedy", "create the run with the user's credential before starting the "+
					"workflow, or give session-core an owner for system-initiated runs")
			return fallback, fmt.Errorf(
				"start run for %s: session-core requires a user-bound credential and a "+
					"Temporal activity can only present a service token: %w", runID, err)
		}
		return fallback, err
	}
	if resp.RunId == "" {
		return fallback, nil
	}
	// Prefer the owner session-core actually persisted over the one we asked for.
	// They differ for exactly the case this exists to support: a run with no human
	// behind it is owned by this service's own principal, and stamping the
	// requested (empty) user instead would make the run's lifecycle events
	// unjoinable to its row.
	owner := resp.OwnerId
	if strings.TrimSpace(owner) == "" {
		owner = userID
	}
	return RunMetadata{RunID: resp.RunId, ThreadID: threadID, OrgID: orgID, UserID: owner, StartedAt: time.Now().UTC()}, nil
}

// ── Activity 2: ExecuteStepLoopActivity + ExecuteStepActivity ────────────────

// executeStepRequest builds the ExecuteStep RPC request from the loop input.
// OrgID (tenant) and UserID (viewer) are threaded so execution-core scopes
// viewer-scoped tools (knowledge-search) to the caller, not the whole org
// (ExecuteStepRequest.user_id, proto field 8). Empty UserID = org-scoped.
func executeStepRequest(input StepLoopInput) *mpv1.ExecuteStepRequest {
	return &mpv1.ExecuteStepRequest{
		RunId:  input.RunID,
		OrgId:  input.OrgID,
		UserId: input.UserID,
	}
}

// executeOneStep performs a single ExecuteStep RPC and maps the response to a
// StepResult. It is the one source of truth for the per-turn fallback/error
// policy shared by the legacy in-activity loop (ExecuteStepLoopActivity) and
// the durable per-turn driver (ExecuteStepActivity): a nil client or an
// Unavailable execution-core yields a non-fatal "pending" step so the caller
// advances to the next turn, while any other error is fatal and returned.
func (a *Activities) executeOneStep(ctx context.Context, req *mpv1.ExecuteStepRequest, stepIndex int) (StepResult, error) {
	pending := StepResult{StepIndex: stepIndex, ToolName: "pending", Completed: false}
	if a.clients == nil || a.clients.ExecutionCore == nil {
		return pending, nil
	}
	resp, err := mpv1.NewExecutionCoreClient(a.clients.ExecutionCore).ExecuteStep(ctx, req)
	if err != nil {
		if status.Code(err) == codes.Unavailable {
			a.logger.Warn("ExecutionCore unavailable", "method", "ExecuteStep")
			return pending, nil
		}
		return StepResult{}, err
	}
	return StepResult{
		StepIndex:     stepIndex,
		Output:        resp.Output,
		Completed:     resp.Status == "completed",
		NeedsApproval: resp.Status == "awaiting_approval",
	}, nil
}

// ExecuteStepLoopActivity runs the entire MaxTurns step loop inside ONE
// activity.
//
// It is NOT resume-safe: a worker crash mid-loop restarts the run from turn 0,
// losing every completed turn and re-spending its work. The durable replacement
// is the workflow-owned per-turn driver (executeStepLoop in cmd/workflows),
// where each turn is its own ExecuteStepActivity checkpoint. This activity stays
// registered so histories recorded before that migration (gated by
// workflow.GetVersion at DefaultVersion) keep replaying deterministically.
func (a *Activities) ExecuteStepLoopActivity(ctx context.Context, input StepLoopInput) (StepLoopOutput, error) {
	if input.MaxTurns <= 0 {
		input.MaxTurns = 10
	}
	req := executeStepRequest(input)
	var steps []StepResult
	for i := range input.MaxTurns {
		select {
		case <-ctx.Done():
			return StepLoopOutput{Steps: steps, Completed: false}, ctx.Err()
		default:
		}
		step, err := a.executeOneStep(ctx, req, i)
		if err != nil {
			return StepLoopOutput{Steps: steps, Completed: false}, err
		}
		steps = append(steps, step)
		if step.Completed || step.NeedsApproval {
			break
		}
	}
	return StepLoopOutput{
		Steps:     steps,
		Completed: len(steps) > 0 && steps[len(steps)-1].Completed,
		Summary:   fmt.Sprintf("executed %d steps for run %s", len(steps), input.RunID),
	}, nil
}

// ExecuteStepActivity performs exactly ONE turn of the step loop against
// execution-core — the durable per-turn checkpoint dispatched once per turn by
// the resume-safe workflow driver (executeStepLoop). Because each turn is its
// own activity, a completed turn is recorded in Temporal event history; a worker
// crash mid-loop replays completed turns and resumes at the interrupted turn
// instead of restarting from turn 0.
func (a *Activities) ExecuteStepActivity(ctx context.Context, in StepInput) (StepResult, error) {
	req := &mpv1.ExecuteStepRequest{RunId: in.RunID, OrgId: in.OrgID, UserId: in.UserID}
	return a.executeOneStep(ctx, req, in.StepIndex)
}

// deterministicSummaryRunes caps the fallback summary. It is a rune count, not a
// byte count: the previous `summary[:512]` sliced by byte and could split a
// multi-byte rune, so Norwegian content landing on the boundary wrote invalid
// UTF-8 into durable memory.
const deterministicSummaryRunes = 512

// truncateRunes cuts s to at most n runes, never mid-rune.
func truncateRunes(s string, n int) string {
	runes := []rune(s)
	if len(runes) <= n {
		return s
	}
	return string(runes[:n])
}

// summarizeMemoryEntries groups entries by thread and joins them.
//
// This is the FALLBACK for [Activities.SummarizeMemoryActivity], not the product:
// joining entries with newlines is concatenation, not consolidation. It decides
// which threads exist and gives each one a summary that is better than nothing
// when inference-core cannot be reached.
func summarizeMemoryEntries(entries []MemoryEntry) []MemoryEntry {
	if len(entries) == 0 {
		return nil
	}
	byThread := make(map[string][]MemoryEntry)
	threadOrder := make([]string, 0)
	for _, entry := range entries {
		threadID := entry.ThreadID
		if threadID == "" {
			// DROP, never bucket under a shared placeholder.
			//
			// This used to collapse every thread-less memory into one "unknown"
			// group, render them into ONE prompt, and write the blended summary
			// back as a single row owned by nobody. Because the search this feeds
			// is org-wide (QueryMemoryEntriesActivity passes no thread and no
			// user), that made this a cross-USER memory blender: one tenant's
			// employees would have had their private memories summarised together
			// and re-indexed as a shared fact.
			//
			// It never fired only because the vector index returns zero rows
			// today (dimension mismatch). Fixing that index without this guard
			// would have switched the blender on. A memory we cannot attribute to
			// a conversation is not consolidatable — skipping it loses nothing
			// that was safe to keep.
			continue
		}
		if _, ok := byThread[threadID]; !ok {
			threadOrder = append(threadOrder, threadID)
		}
		byThread[threadID] = append(byThread[threadID], entry)
	}
	sort.Strings(threadOrder)

	consolidated := make([]MemoryEntry, 0, len(threadOrder))
	for _, threadID := range threadOrder {
		threadEntries := byThread[threadID]
		parts := make([]string, 0, len(threadEntries))
		latest := threadEntries[0].CreatedAt
		orgID := threadEntries[0].OrgID
		for _, entry := range threadEntries {
			parts = append(parts, strings.TrimSpace(entry.Content))
			if entry.CreatedAt.After(latest) {
				latest = entry.CreatedAt
			}
		}
		summary := truncateRunes(strings.Join(parts, "\n"), deterministicSummaryRunes)
		consolidated = append(consolidated, MemoryEntry{
			ID:        fmt.Sprintf("%s-consolidated", threadID),
			OrgID:     orgID,
			ThreadID:  threadID,
			Content:   summary,
			CreatedAt: latest,
		})
	}
	return consolidated
}

// CompleteRunActivity publishes the run's RUN_COMPLETED lifecycle envelope,
// stamped with the run's retention posture.
//
// The summary is the run's own output — customer content — and mp.v1.run.*.event
// is captured by the JetStream stream MODEL_PLANE_RUN_EVENTS (48h, file-backed),
// so emitting it puts that content on disk. It is therefore included ONLY under
// an explicitly attested durable posture. A Zero Data Retention run emits
// nothing at all (natsx suppresses the envelope); an unattested run still emits
// the lifecycle fact — insight-core's run counters depend on it — but without
// the content, because "nobody declared a posture" is not permission to retain.
func (a *Activities) CompleteRunActivity(_ context.Context, input CompletionInput) error {
	payload := map[string]any{"run_id": input.RunID}
	if input.Retention.AllowsContent() {
		payload["summary"] = input.Summary
	}
	a.publishRunEvent(input.RunID, input.OrgID, input.UserID, "RUN_COMPLETED", "completed", input.Retention, payload)
	return nil
}

// FailRunActivity publishes RUN_FAILED. The reason string can carry downstream
// error text, so it is gated on the retention posture exactly like the summary
// in [Activities.CompleteRunActivity].
func (a *Activities) FailRunActivity(_ context.Context, input FailureInput) error {
	payload := map[string]any{"run_id": input.RunID}
	if input.Retention.AllowsContent() {
		payload["reason"] = input.Reason
	}
	a.publishRunEvent(input.RunID, input.OrgID, input.UserID, "RUN_FAILED", "failed", input.Retention, payload)
	return nil
}

func (a *Activities) QueryMemoryEntriesActivity(ctx context.Context, input MemoryQueryInput) ([]MemoryEntry, error) {
	if a.clients == nil || a.clients.LettaBridge == nil {
		return nil, status.Error(codes.Unavailable, "letta-bridge unavailable")
	}
	if input.MaxItems <= 0 {
		input.MaxItems = 100
	}
	resp, err := mpv1.NewMemoryServiceClient(a.clients.LettaBridge).SearchMemory(ctx, &mpv1.SearchMemoryRequest{
		OrgId:       input.OrgID,
		TopicFilter: []string{"MEMORY"},
		Limit:       uint32(input.MaxItems),
	})
	if err != nil {
		return nil, err
	}
	entries := make([]MemoryEntry, 0, len(resp.Entries))
	for _, entry := range resp.Entries {
		if entry == nil {
			continue
		}
		createdAt := time.Time{}
		if entry.UpdatedAt != nil {
			createdAt = entry.UpdatedAt.AsTime()
		}
		if !input.Since.IsZero() && createdAt.Before(input.Since) {
			continue
		}
		entries = append(entries, MemoryEntry{
			ID:        entry.MemoryId,
			OrgID:     input.OrgID,
			Content:   entry.Content,
			ThreadID:  entry.ThreadId,
			CreatedAt: createdAt,
		})
	}
	return entries, nil
}

func (a *Activities) WriteConsolidatedMemoryActivity(ctx context.Context, input WriteMemoryInput) error {
	if a.clients == nil || a.clients.LettaBridge == nil {
		return status.Error(codes.Unavailable, "letta-bridge unavailable")
	}
	client := mpv1.NewMemoryServiceClient(a.clients.LettaBridge)
	for _, entry := range input.Entries {
		_, err := client.IndexMemory(ctx, &mpv1.IndexMemoryRequest{
			ThreadId: entry.ThreadID,
			Topic:    "MEMORY",
			Content:  entry.Content,
			OrgId:    entry.OrgID,
		})
		if err != nil {
			return err
		}
	}
	return nil
}

func (a *Activities) ValidateSkillBundleActivity(ctx context.Context, input SkillValidationInput) (SkillValidationOutput, error) {
	if a.clients == nil || a.clients.CapabilityCore == nil {
		return SkillValidationOutput{}, status.Error(codes.Unavailable, "capability-core unavailable")
	}
	// capability-core's skill RPCs carry no org_id, so the tenant must reach the
	// outbound interceptor through the context or no org-bound token can be minted.
	ctx = servicecred.WithOrg(ctx, input.OrgID)
	resp, err := mpv1.NewCapabilityCoreClient(a.clients.CapabilityCore).ValidateSkillBundle(ctx, &mpv1.ValidateSkillBundleRequest{SkillId: input.SkillID})
	if err != nil {
		return SkillValidationOutput{}, err
	}
	return SkillValidationOutput{Valid: resp.Valid, Errors: resp.Errors}, nil
}

func (a *Activities) RunPromotionGateActivity(ctx context.Context, input PromotionGateInput) (PromotionGateOutput, error) {
	if a.clients == nil || a.clients.CapabilityCore == nil {
		return PromotionGateOutput{}, status.Error(codes.Unavailable, "capability-core unavailable")
	}
	ctx = servicecred.WithOrg(ctx, input.OrgID)
	resp, err := mpv1.NewCapabilityCoreClient(a.clients.CapabilityCore).CheckSkillPromotion(ctx, &mpv1.CheckSkillPromotionRequest{
		SkillId:   input.SkillID,
		FromScope: input.FromScope,
		ToScope:   input.ToScope,
	})
	if err != nil {
		return PromotionGateOutput{}, err
	}
	return PromotionGateOutput{Passed: resp.Passed, Checks: resp.Checks}, nil
}

func (a *Activities) UpdateRegistryActivity(ctx context.Context, input RegistryUpdateInput) error {
	if a.clients == nil || a.clients.CapabilityCore == nil {
		return status.Error(codes.Unavailable, "capability-core unavailable")
	}
	ctx = servicecred.WithOrg(ctx, input.OrgID)
	_, err := mpv1.NewCapabilityCoreClient(a.clients.CapabilityCore).PromoteSkill(ctx, &mpv1.PromoteSkillRequest{
		SkillId:   input.SkillID,
		FromScope: input.FromScope,
		ToScope:   input.NewScope,
	})
	return err
}

// ── Feedback aggregation (feedback → skill-promotion loop) ───────────────────

type FeedbackAggregateInput struct {
	// Minimum ratings a skill needs before it's eligible. Default 5.
	MinSamples int
	// Good-ratio required to promote (0..1). Default 0.8.
	PromoteThreshold float64
}
type FeedbackAggregateOutput struct {
	Candidates []SkillPromotionCandidate
}

// AggregateFeedbackActivity reads the durable operator-rating store and returns
// the skills whose feedback has earned a promotion. The FeedbackPromotionWorkflow
// runs this, then launches SkillPromotionWorkflow for each candidate.
func (a *Activities) AggregateFeedbackActivity(ctx context.Context, input FeedbackAggregateInput) (FeedbackAggregateOutput, error) {
	if a.feedbackStore == nil {
		return FeedbackAggregateOutput{}, nil
	}
	minSamples := input.MinSamples
	if minSamples <= 0 {
		minSamples = 5
	}
	threshold := input.PromoteThreshold
	if threshold <= 0 {
		threshold = 0.8
	}
	candidates, err := a.feedbackStore.Candidates(ctx, minSamples, threshold)
	if err != nil {
		return FeedbackAggregateOutput{}, err
	}
	return FeedbackAggregateOutput{Candidates: promotionCandidates(candidates)}, nil
}
