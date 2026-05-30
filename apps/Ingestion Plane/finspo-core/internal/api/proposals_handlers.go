package api

import (
	"context"
	"errors"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/google/uuid"

	"github.com/triodelab/finspo/internal/auth"
	"github.com/triodelab/finspo/internal/events"
	"github.com/triodelab/finspo/internal/store"
	"github.com/triodelab/finspo/internal/sync"
)

// ProposalStore is the narrow contract the proposals handlers require.
// Implemented by *store.Proposals.
type ProposalStore interface {
	Create(ctx context.Context, in store.CreateProposalInput) (store.Proposal, error)
	Get(ctx context.Context, id uuid.UUID) (store.Proposal, error)
	ListByOrg(ctx context.Context, organizationID, status string, limit int) ([]store.Proposal, error)
	Decide(ctx context.Context, id uuid.UUID, newStatus, decidedBy, notes string) (store.Proposal, error)
}

// AuditWriter is the narrow contract for the audit_log writer.
// Implemented by *store.Audit.
type AuditWriter interface {
	Write(ctx context.Context, in store.AuditInput) (store.AuditEntry, error)
}

// ProposalExecutor runs an approved proposal's destructive action.
// Implemented by *sync.Executor.
type ProposalExecutor interface {
	Enabled() bool
	ExecuteProposal(ctx context.Context, proposalID uuid.UUID, actor string) (sync.ExecResult, error)
}

// LifecycleEmitter publishes proposal lifecycle events. *events.Publisher
// satisfies the Publish half; the emitter wraps it with the subject map so a
// nil publisher is a no-op.
type LifecycleEmitter struct {
	Publisher interface {
		Publish(subject string, payload any) error
	}
	Subjects events.Subjects
}

func (e *LifecycleEmitter) emit(prop store.Proposal, subject, actor string) {
	if e == nil || e.Publisher == nil || subject == "" {
		return
	}
	_ = e.Publisher.Publish(subject, events.ProposalLifecycle{
		OrganizationID: prop.OrganizationID,
		ProposalID:     prop.ID.String(),
		Kind:           prop.Kind,
		Status:         prop.Status,
		Actor:          actor,
		ItemCount:      len(prop.ItemPKs),
		OccurredAt:     time.Now().UTC(),
	})
}

type createProposalRequest struct {
	Kind    string      `json:"kind"`
	Reason  string      `json:"reason"`
	ItemPKs []uuid.UUID `json:"item_pks"`
	Notes   string      `json:"notes,omitempty"`
}

type decideProposalRequest struct {
	Notes string `json:"notes,omitempty"`
}

func registerProposalRoutes(g fiber.Router, proposals ProposalStore, audit AuditWriter, executor ProposalExecutor, emitter *LifecycleEmitter, subjects events.Subjects) {
	if proposals == nil {
		return
	}
	g.Post("/proposals", createProposalHandler(proposals, audit, emitter, subjects))
	g.Get("/proposals", listProposalsHandler(proposals))
	g.Get("/proposals/:id", getProposalHandler(proposals))
	g.Post("/proposals/:id/approve", decideProposalHandler(proposals, audit, emitter, subjects, store.ProposalStatusApproved))
	g.Post("/proposals/:id/reject", decideProposalHandler(proposals, audit, emitter, subjects, store.ProposalStatusRejected))
	g.Post("/proposals/:id/execute", executeProposalHandler(proposals, executor))
}

func createProposalHandler(proposals ProposalStore, audit AuditWriter, emitter *LifecycleEmitter, subjects events.Subjects) fiber.Handler {
	return func(c *fiber.Ctx) error {
		var body createProposalRequest
		if err := c.BodyParser(&body); err != nil {
			return clientError(c, fiber.StatusBadRequest, "invalid JSON body")
		}
		body.Kind = strings.TrimSpace(strings.ToLower(body.Kind))
		body.Reason = strings.TrimSpace(body.Reason)
		if body.Reason == "" {
			return clientError(c, fiber.StatusBadRequest, "reason is required")
		}
		if body.Kind != store.ProposalKindDelete && body.Kind != store.ProposalKindArchive {
			return clientError(c, fiber.StatusBadRequest, "kind must be 'delete' or 'archive'")
		}
		if len(body.ItemPKs) == 0 {
			return clientError(c, fiber.StatusBadRequest, "item_pks must include at least one item")
		}

		actor := proposedBy(c)
		orgID := auth.OrganizationID(c)
		prop, err := proposals.Create(c.UserContext(), store.CreateProposalInput{
			OrganizationID: orgID,
			ProposedBy:     actor,
			Kind:           body.Kind,
			Reason:         body.Reason,
			ItemPKs:        body.ItemPKs,
			Notes:          body.Notes,
		})
		if err != nil {
			return serverError(c, "create proposal", err)
		}

		if audit != nil {
			_, _ = audit.Write(c.UserContext(), store.AuditInput{
				OrganizationID: orgID,
				Actor:          actor,
				Action:         "proposal.created",
				TargetKind:     "proposal",
				TargetID:       prop.ID.String(),
				Payload: fiber.Map{
					"kind":   prop.Kind,
					"reason": prop.Reason,
					"items":  prop.ItemPKs,
					"notes":  prop.Notes,
				},
			})
		}
		emitter.emit(prop, subjects.ProposalCreated(), actor)
		return c.Status(fiber.StatusCreated).JSON(fiber.Map{"success": true, "data": prop})
	}
}

func listProposalsHandler(proposals ProposalStore) fiber.Handler {
	return func(c *fiber.Ctx) error {
		status := strings.TrimSpace(strings.ToLower(c.Query("status", "")))
		limit := boundedIntQuery(c, "limit", 100, 1, 500)
		out, err := proposals.ListByOrg(c.UserContext(), auth.OrganizationID(c), status, limit)
		if err != nil {
			return serverError(c, "list proposals", err)
		}
		return c.JSON(fiber.Map{
			"success": true,
			"data": fiber.Map{
				"count":     len(out),
				"proposals": out,
			},
		})
	}
}

func getProposalHandler(proposals ProposalStore) fiber.Handler {
	return func(c *fiber.Ctx) error {
		id, err := uuid.Parse(c.Params("id"))
		if err != nil {
			return clientError(c, fiber.StatusBadRequest, "invalid proposal id")
		}
		prop, err := proposals.Get(c.UserContext(), id)
		if errors.Is(err, store.ErrNotFound) {
			return clientError(c, fiber.StatusNotFound, "proposal not found")
		}
		if err != nil {
			return serverError(c, "get proposal", err)
		}
		if prop.OrganizationID != auth.OrganizationID(c) {
			return clientError(c, fiber.StatusNotFound, "proposal not found")
		}
		return c.JSON(fiber.Map{"success": true, "data": prop})
	}
}

func decideProposalHandler(proposals ProposalStore, audit AuditWriter, emitter *LifecycleEmitter, subjects events.Subjects, newStatus string) fiber.Handler {
	return func(c *fiber.Ctx) error {
		id, err := uuid.Parse(c.Params("id"))
		if err != nil {
			return clientError(c, fiber.StatusBadRequest, "invalid proposal id")
		}
		var body decideProposalRequest
		_ = c.BodyParser(&body) // notes are optional; ignore parse errors on empty bodies
		actor := proposedBy(c)
		orgID := auth.OrganizationID(c)

		// Tenant guard BEFORE we mutate the row — using Get() instead of
		// trusting the proposal record on Decide() means a wrong-org caller
		// cannot even see the proposal exists.
		existing, err := proposals.Get(c.UserContext(), id)
		if errors.Is(err, store.ErrNotFound) {
			return clientError(c, fiber.StatusNotFound, "proposal not found")
		}
		if err != nil {
			return serverError(c, "get proposal", err)
		}
		if existing.OrganizationID != orgID {
			return clientError(c, fiber.StatusNotFound, "proposal not found")
		}

		prop, err := proposals.Decide(c.UserContext(), id, newStatus, actor, body.Notes)
		if errors.Is(err, store.ErrInvalidTransition) {
			return clientError(c, fiber.StatusConflict, "proposal is no longer pending")
		}
		if errors.Is(err, store.ErrNotFound) {
			return clientError(c, fiber.StatusNotFound, "proposal not found")
		}
		if err != nil {
			return serverError(c, "decide proposal", err)
		}

		if audit != nil {
			_, _ = audit.Write(c.UserContext(), store.AuditInput{
				OrganizationID: orgID,
				Actor:          actor,
				Action:         "proposal." + newStatus,
				TargetKind:     "proposal",
				TargetID:       prop.ID.String(),
				Payload: fiber.Map{
					"kind":  prop.Kind,
					"notes": body.Notes,
				},
			})
		}
		emitter.emit(prop, subjects.ForProposalStatus(newStatus), actor)
		return c.JSON(fiber.Map{"success": true, "data": prop})
	}
}

// executeProposalHandler triggers the destructive action for an approved
// proposal. Execution is gated three ways: the FINSPO_ALLOW_EXECUTION
// kill-switch (Executor.Enabled), the proposal must be approved, and the
// caller must own the org. The executor itself emits the executed/failed
// lifecycle events and per-item audit rows.
func executeProposalHandler(proposals ProposalStore, executor ProposalExecutor) fiber.Handler {
	return func(c *fiber.Ctx) error {
		if executor == nil || !executor.Enabled() {
			return clientError(c, fiber.StatusServiceUnavailable, "execution is disabled (set FINSPO_ALLOW_EXECUTION=true)")
		}
		id, err := uuid.Parse(c.Params("id"))
		if err != nil {
			return clientError(c, fiber.StatusBadRequest, "invalid proposal id")
		}

		// Tenant guard before any destructive work.
		existing, err := proposals.Get(c.UserContext(), id)
		if errors.Is(err, store.ErrNotFound) {
			return clientError(c, fiber.StatusNotFound, "proposal not found")
		}
		if err != nil {
			return serverError(c, "get proposal", err)
		}
		if existing.OrganizationID != auth.OrganizationID(c) {
			return clientError(c, fiber.StatusNotFound, "proposal not found")
		}

		res, err := executor.ExecuteProposal(c.UserContext(), id, proposedBy(c))
		if errors.Is(err, sync.ErrNotApproved) {
			return clientError(c, fiber.StatusConflict, "proposal must be approved before execution")
		}
		if errors.Is(err, sync.ErrExecutionDisabled) {
			return clientError(c, fiber.StatusServiceUnavailable, "execution is disabled")
		}
		if errors.Is(err, sync.ErrArchiveNotConfigured) {
			return clientError(c, fiber.StatusBadRequest, "archive folder not configured (set FINSPO_ARCHIVE_FOLDER_ID)")
		}
		if err != nil {
			return serverError(c, "execute proposal", err)
		}
		return c.JSON(fiber.Map{"success": true, "data": res})
	}
}

// proposedBy uses the org id header for now; we can swap in a user-id header
// once velion is forwarding one.
func proposedBy(c *fiber.Ctx) string {
	if u := strings.TrimSpace(c.Get("X-User-ID")); u != "" {
		return u
	}
	return "org:" + auth.OrganizationID(c)
}
