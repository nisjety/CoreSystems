// Package activities defines Temporal activity implementations for orchestrator-core.
package activities

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"sort"
	"strings"
	"time"

	"github.com/google/uuid"
	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/pkg/natsx"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/grpcclient"
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
}
type StepLoopOutput struct {
	Steps     []StepResult
	Completed bool
	Summary   string
}
type CompletionInput struct{ RunID, OrgID, UserID, Summary string }
type FailureInput struct{ RunID, OrgID, UserID, Reason string }
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
type SkillValidationInput struct{ SkillID string }
type SkillValidationOutput struct {
	Valid  bool
	Errors []string
}
type PromotionGateInput struct{ SkillID, FromScope, ToScope string }
type PromotionGateOutput struct {
	Passed bool
	Checks []string
}
type RegistryUpdateInput struct{ SkillID, FromScope, NewScope string }

// ── Struct + constructor ─────────────────────────────────────────────────────

type Activities struct {
	logger    *slog.Logger
	clients   *grpcclient.Clients
	publisher *natsx.Publisher
	feedback  *FeedbackStore
}

func NewActivities(logger *slog.Logger, clients *grpcclient.Clients) *Activities {
	return &Activities{logger: logger, clients: clients}
}

// SetPublisher wires a NATS publisher for emitting run lifecycle envelopes.
func (a *Activities) SetPublisher(p *natsx.Publisher) { a.publisher = p }

// SetFeedbackStore wires the operator-rating accumulator that backs the
// feedback → skill-promotion loop.
func (a *Activities) SetFeedbackStore(f *FeedbackStore) { a.feedback = f }

func (a *Activities) publishRunEvent(runID, orgID, userID, eventType, idemSuffix string, payload any) {
	if a.publisher == nil {
		return
	}
	data, err := json.Marshal(payload)
	if err != nil {
		a.logger.Error("marshal run event payload", "err", err, "event", eventType)
		return
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

func (a *Activities) StartRunActivity(ctx context.Context, runID, threadID, orgID, userID string) (RunMetadata, error) {
	fallback := RunMetadata{RunID: runID, ThreadID: threadID, OrgID: orgID, UserID: userID, StartedAt: time.Now().UTC()}
	if a.clients == nil || a.clients.SessionCore == nil {
		a.logger.Warn("downstream unavailable, skipping", "method", "StartRun")
		return fallback, nil
	}
	resp, err := mpv1.NewSessionCoreClient(a.clients.SessionCore).StartRun(ctx, &mpv1.StartRunRequest{
		ThreadId: threadID,
		OrgId:    orgID,
		UserId:   userID,
	})
	if err != nil {
		if status.Code(err) == codes.Unavailable {
			a.logger.Warn("SessionCore unavailable", "method", "StartRun")
			return fallback, nil
		}
		return fallback, err
	}
	if resp.RunId == "" {
		return fallback, nil
	}
	return RunMetadata{RunID: resp.RunId, ThreadID: threadID, OrgID: orgID, UserID: userID, StartedAt: time.Now().UTC()}, nil
}

// ── Activity 2: ExecuteStepLoopActivity ──────────────────────────────────────

func (a *Activities) ExecuteStepLoopActivity(ctx context.Context, input StepLoopInput) (StepLoopOutput, error) {
	if input.MaxTurns <= 0 {
		input.MaxTurns = 10
	}
	var steps []StepResult
	for i := range input.MaxTurns {
		select {
		case <-ctx.Done():
			return StepLoopOutput{Steps: steps, Completed: false}, ctx.Err()
		default:
		}
		step := StepResult{StepIndex: i, ToolName: "pending", Completed: false}
		if a.clients != nil && a.clients.ExecutionCore != nil {
			resp, err := mpv1.NewExecutionCoreClient(a.clients.ExecutionCore).ExecuteStep(ctx, &mpv1.ExecuteStepRequest{
				RunId: input.RunID,
			})
			if err != nil {
				if status.Code(err) == codes.Unavailable {
					a.logger.Warn("ExecutionCore unavailable", "method", "ExecuteStep")
				} else {
					return StepLoopOutput{Steps: steps, Completed: false}, err
				}
			} else {
				step = StepResult{
					StepIndex:     i,
					Output:        resp.Output,
					Completed:     resp.Status == "completed",
					NeedsApproval: resp.Status == "awaiting_approval",
				}
			}
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

func summarizeMemoryEntries(entries []MemoryEntry) []MemoryEntry {
	if len(entries) == 0 {
		return nil
	}
	byThread := make(map[string][]MemoryEntry)
	threadOrder := make([]string, 0)
	for _, entry := range entries {
		threadID := entry.ThreadID
		if threadID == "" {
			threadID = "unknown"
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
		summary := strings.Join(parts, "\n")
		if len(summary) > 512 {
			summary = summary[:512]
		}
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

func (a *Activities) CompleteRunActivity(_ context.Context, input CompletionInput) error {
	a.publishRunEvent(input.RunID, input.OrgID, input.UserID, "RUN_COMPLETED", "completed", map[string]any{
		"run_id":  input.RunID,
		"summary": input.Summary,
	})
	return nil
}

func (a *Activities) FailRunActivity(_ context.Context, input FailureInput) error {
	a.publishRunEvent(input.RunID, input.OrgID, input.UserID, "RUN_FAILED", "failed", map[string]any{
		"run_id": input.RunID,
		"reason": input.Reason,
	})
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

func (a *Activities) SummarizeMemoryActivity(_ context.Context, input ConsolidationInput) (ConsolidationOutput, error) {
	consolidated := summarizeMemoryEntries(input.Entries)
	return ConsolidationOutput{
		ConsolidatedEntries: consolidated,
		Summary:             fmt.Sprintf("consolidated %d entries into %d summaries", len(input.Entries), len(consolidated)),
	}, nil
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

// AggregateFeedbackActivity reads the operator-rating accumulator and returns
// the skills whose feedback has earned a promotion. The FeedbackPromotionWorkflow
// runs this, then launches SkillPromotionWorkflow for each candidate.
func (a *Activities) AggregateFeedbackActivity(_ context.Context, input FeedbackAggregateInput) (FeedbackAggregateOutput, error) {
	if a.feedback == nil {
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
	return FeedbackAggregateOutput{Candidates: a.feedback.Candidates(minSamples, threshold)}, nil
}
