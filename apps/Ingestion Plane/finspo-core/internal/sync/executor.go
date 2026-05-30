package sync

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/rs/zerolog"

	"github.com/triodelab/finspo/internal/events"
	"github.com/triodelab/finspo/internal/store"
)

// ErrExecutionDisabled is returned when execution is attempted while the
// FINSPO_ALLOW_EXECUTION kill-switch is off.
var ErrExecutionDisabled = errors.New("executor: execution is disabled (FINSPO_ALLOW_EXECUTION=false)")

// ErrNotApproved is returned when a proposal is not in the approved state.
var ErrNotApproved = errors.New("executor: proposal is not approved")

// ErrArchiveNotConfigured is returned when an archive proposal is executed but
// no archive destination folder is configured.
var ErrArchiveNotConfigured = errors.New("executor: archive folder not configured")

// Mutator performs the destructive Graph operations. Implemented by
// *sharepoint.MutationClient.
type Mutator interface {
	DeleteItem(ctx context.Context, organizationID, driveID, itemID string) error
	MoveItem(ctx context.Context, organizationID, driveID, itemID, destFolderID string) error
}

// ProposalExecStore is the proposal persistence surface the executor needs.
type ProposalExecStore interface {
	Get(ctx context.Context, id uuid.UUID) (store.Proposal, error)
	SetExecutionResult(ctx context.Context, id uuid.UUID, newStatus, failureReason string) (store.Proposal, error)
}

// ItemExecStore resolves item PKs to Graph targets and records local deletes.
type ItemExecStore interface {
	ResolveForExecution(ctx context.Context, itemPK uuid.UUID) (store.ExecTarget, error)
	SoftDeleteByPK(ctx context.Context, itemPK uuid.UUID) error
}

// Executor acts on a human-approved proposal, performing the destructive Graph
// operation for each item, then recording the outcome and emitting audit +
// events. It NEVER acts on its own: the operator must (1) flip
// AllowExecution on and (2) explicitly trigger execution of an approved
// proposal.
type Executor struct {
	proposals   ProposalExecStore
	items       ItemExecStore
	mutator     Mutator
	audit       AuditWriter
	publisher   Publisher
	subjects    events.Subjects
	logger      zerolog.Logger
	allow       bool
	archiveDest string
}

// AuditWriter is the audit surface used by the executor (matches store.Audit).
type AuditWriter interface {
	Write(ctx context.Context, in store.AuditInput) (store.AuditEntry, error)
}

type ExecutorConfig struct {
	Proposals      ProposalExecStore
	Items          ItemExecStore
	Mutator        Mutator
	Audit          AuditWriter
	Publisher      Publisher
	Subjects       events.Subjects
	Logger         zerolog.Logger
	AllowExecution bool
	ArchiveFolder  string
}

func NewExecutor(cfg ExecutorConfig) *Executor {
	return &Executor{
		proposals:   cfg.Proposals,
		items:       cfg.Items,
		mutator:     cfg.Mutator,
		audit:       cfg.Audit,
		publisher:   cfg.Publisher,
		subjects:    cfg.Subjects,
		logger:      cfg.Logger,
		allow:       cfg.AllowExecution,
		archiveDest: strings.TrimSpace(cfg.ArchiveFolder),
	}
}

// Enabled reports whether destructive execution is turned on.
func (x *Executor) Enabled() bool { return x.allow }

// ItemOutcome captures the per-item result of an execution run.
type ItemOutcome struct {
	ItemPK string `json:"item_pk"`
	ItemID string `json:"item_id,omitempty"`
	Path   string `json:"path,omitempty"`
	OK     bool   `json:"ok"`
	Error  string `json:"error,omitempty"`
}

// ExecResult summarizes one ExecuteProposal run.
type ExecResult struct {
	ProposalID string        `json:"proposal_id"`
	Kind       string        `json:"kind"`
	Status     string        `json:"status"`
	Succeeded  int           `json:"succeeded"`
	Failed     int           `json:"failed"`
	Outcomes   []ItemOutcome `json:"outcomes"`
}

// ExecuteProposal runs the destructive action for every item in an approved
// proposal. It is best-effort across items: one item's failure does not abort
// the rest, but if ANY item fails the proposal is marked failed (with a
// summary) so an operator can review and retry.
func (x *Executor) ExecuteProposal(ctx context.Context, proposalID uuid.UUID, actor string) (ExecResult, error) {
	if !x.allow {
		return ExecResult{}, ErrExecutionDisabled
	}

	prop, err := x.proposals.Get(ctx, proposalID)
	if err != nil {
		return ExecResult{}, fmt.Errorf("load proposal: %w", err)
	}
	if prop.Status != store.ProposalStatusApproved {
		return ExecResult{}, ErrNotApproved
	}
	if prop.Kind == store.ProposalKindArchive && x.archiveDest == "" {
		// Fail fast without touching Graph; record the failure so the
		// operator sees why nothing happened.
		_, _ = x.proposals.SetExecutionResult(ctx, prop.ID, store.ProposalStatusFailed, ErrArchiveNotConfigured.Error())
		return ExecResult{}, ErrArchiveNotConfigured
	}

	result := ExecResult{ProposalID: prop.ID.String(), Kind: prop.Kind}

	for _, itemPK := range prop.ItemPKs {
		outcome := x.executeItem(ctx, prop, itemPK)
		result.Outcomes = append(result.Outcomes, outcome)
		if outcome.OK {
			result.Succeeded++
		} else {
			result.Failed++
		}
		x.writeItemAudit(ctx, prop, actor, outcome)
	}

	// Decide final proposal status.
	finalStatus := store.ProposalStatusExecuted
	failureReason := ""
	if result.Failed > 0 {
		finalStatus = store.ProposalStatusFailed
		failureReason = fmt.Sprintf("%d of %d items failed", result.Failed, len(prop.ItemPKs))
	}

	updated, err := x.proposals.SetExecutionResult(ctx, prop.ID, finalStatus, failureReason)
	if err != nil {
		return result, fmt.Errorf("record execution result: %w", err)
	}
	result.Status = updated.Status

	x.publishLifecycle(prop, updated.Status, actor, len(prop.ItemPKs), result.Succeeded, result.Failed, failureReason)

	x.logger.Info().
		Str("proposal_id", prop.ID.String()).
		Str("kind", prop.Kind).
		Str("status", updated.Status).
		Int("succeeded", result.Succeeded).
		Int("failed", result.Failed).
		Msg("proposal execution complete")

	return result, nil
}

func (x *Executor) executeItem(ctx context.Context, prop store.Proposal, itemPK uuid.UUID) ItemOutcome {
	out := ItemOutcome{ItemPK: itemPK.String()}

	target, err := x.items.ResolveForExecution(ctx, itemPK)
	if err != nil {
		out.Error = "resolve item: " + err.Error()
		return out
	}
	out.ItemID = target.ItemID
	out.Path = target.Path

	if target.IsFolder {
		out.Error = "refusing to act on a folder target"
		return out
	}

	switch prop.Kind {
	case store.ProposalKindDelete:
		if err := x.mutator.DeleteItem(ctx, target.OrganizationID, target.DriveID, target.ItemID); err != nil {
			out.Error = "graph delete: " + err.Error()
			return out
		}
		if err := x.items.SoftDeleteByPK(ctx, itemPK); err != nil && !errors.Is(err, store.ErrNotFound) {
			// Graph delete succeeded; local bookkeeping failed. The next
			// delta tombstone will reconcile, so log but treat as success.
			x.logger.Warn().Err(err).Str("item_pk", itemPK.String()).Msg("graph delete ok but local soft-delete failed")
		}
		out.OK = true

	case store.ProposalKindArchive:
		if err := x.mutator.MoveItem(ctx, target.OrganizationID, target.DriveID, target.ItemID, x.archiveDest); err != nil {
			out.Error = "graph move: " + err.Error()
			return out
		}
		// Do NOT soft-delete on archive — the item still exists, just relocated.
		// The next delta updates its path.
		out.OK = true

	default:
		out.Error = "unknown proposal kind: " + prop.Kind
	}
	return out
}

func (x *Executor) writeItemAudit(ctx context.Context, prop store.Proposal, actor string, outcome ItemOutcome) {
	if x.audit == nil {
		return
	}
	action := "proposal.item.executed"
	if !outcome.OK {
		action = "proposal.item.failed"
	}
	_, _ = x.audit.Write(ctx, store.AuditInput{
		OrganizationID: prop.OrganizationID,
		Actor:          actor,
		Action:         action,
		TargetKind:     "item",
		TargetID:       outcome.ItemPK,
		Payload: map[string]any{
			"proposal_id": prop.ID.String(),
			"kind":        prop.Kind,
			"item_id":     outcome.ItemID,
			"path":        outcome.Path,
			"ok":          outcome.OK,
			"error":       outcome.Error,
		},
	})
}

func (x *Executor) publishLifecycle(prop store.Proposal, status, actor string, itemCount, succeeded, failed int, failureReason string) {
	if x.publisher == nil {
		return
	}
	subject := x.subjects.ForProposalStatus(status)
	if subject == "" {
		return
	}
	_ = x.publisher.Publish(subject, events.ProposalLifecycle{
		OrganizationID: prop.OrganizationID,
		ProposalID:     prop.ID.String(),
		Kind:           prop.Kind,
		Status:         status,
		Actor:          actor,
		ItemCount:      itemCount,
		ItemsSucceeded: succeeded,
		ItemsFailed:    failed,
		FailureReason:  failureReason,
		OccurredAt:     time.Now().UTC(),
	})
}
