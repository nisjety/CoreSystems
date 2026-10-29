package api

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog"

	"github.com/triodelab/anbud/internal/auth"
	"github.com/triodelab/anbud/internal/incumbency"
	"github.com/triodelab/anbud/internal/normalize"
	"github.com/triodelab/anbud/internal/profilesuggest"
	"github.com/triodelab/anbud/internal/workspace"
)

// Workspace is what an organisation keeps while it works the tender list. The store is workspace.Store.
type Workspace interface {
	GetProfile(ctx context.Context, org string) (workspace.Profile, error)
	PutProfile(ctx context.Context, org, by string, profile json.RawMessage) (workspace.Profile, error)
	ListPipeline(ctx context.Context, org string) ([]workspace.PipelineItem, error)
	SetStage(ctx context.Context, org, by, noticeID, stage string) (workspace.PipelineItem, error)
	ListSearches(ctx context.Context, org string) ([]workspace.SavedSearch, error)
	SaveSearch(ctx context.Context, org, by, name string, query json.RawMessage) (workspace.SavedSearch, error)
	DeleteSearch(ctx context.Context, org string, id int64) error
	PutProfileIfAbsent(ctx context.Context, org, by string, profile json.RawMessage) (workspace.Profile, bool, error)
	Seen(ctx context.Context, org string) (time.Time, bool, error)
	MarkSeen(ctx context.Context, org, by string) (time.Time, error)
}

// WinSource is where a suggested profile is drawn from: the awards the notice store holds.
type WinSource interface {
	Awards(ctx context.Context) ([]incumbency.Award, error)
}

// The routes are POST, as the rest of the API is, so the gateway forwards them the same way. Each needs
// X-Org-ID (the organisation). A change also needs X-User-ID: it is recorded as the actor of the change.
func registerWorkspace(r fiber.Router, svc Workspace, wins WinSource, logger *zerolog.Logger) {
	g := r.Group("/workspace", auth.RequireOrganization())
	g.Post("/profile/read", profileReadHandler(svc, logger))
	g.Post("/profile/write", profileWriteHandler(svc, logger))
	if wins != nil {
		g.Post("/profile/suggest", profileSuggestHandler(svc, wins, logger))
	}
	g.Post("/pipeline/read", pipelineReadHandler(svc, logger))
	g.Post("/pipeline/write", pipelineWriteHandler(svc, logger))
	g.Post("/searches/read", searchesReadHandler(svc, logger))
	g.Post("/searches/write", searchWriteHandler(svc, logger))
	g.Post("/searches/delete", searchDeleteHandler(svc, logger))
	g.Post("/seen/read", seenReadHandler(svc, logger))
	g.Post("/seen/write", seenWriteHandler(svc, logger))
}

func profileReadHandler(svc Workspace, logger *zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		p, err := svc.GetProfile(c.UserContext(), auth.OrganizationID(c))
		if errors.Is(err, workspace.ErrNotFound) {
			// Never saved is not a failure: the page starts empty.
			return c.JSON(fiber.Map{"data": nil})
		}
		if err != nil {
			return workspaceError(c, err, logger)
		}
		return c.JSON(fiber.Map{"data": p})
	}
}

func profileWriteHandler(svc Workspace, logger *zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		actor := auth.UserID(c)
		if actor == "" {
			return writeError(c, fiber.StatusUnauthorized, "user_required", "the request names no user")
		}
		if tooLarge(c) {
			return writeError(c, fiber.StatusRequestEntityTooLarge, "payload_too_large", "the request body is too large")
		}
		var req struct {
			Profile json.RawMessage `json:"profile"`
		}
		if err := json.Unmarshal(c.BodyRaw(), &req); err != nil || len(req.Profile) == 0 {
			return writeError(c, fiber.StatusBadRequest, "bad_request", "send the profile as a JSON object in the field profile")
		}
		p, err := svc.PutProfile(c.UserContext(), auth.OrganizationID(c), actor, req.Profile)
		if err != nil {
			return workspaceError(c, err, logger)
		}
		return c.JSON(fiber.Map{"data": p})
	}
}

// profileSuggestHandler drafts a profile from the awards the organisation won, and saves it only when the
// organisation has no profile yet. The organisation number is the gateway's, from the organisation record; the
// awards are the company's own, so the draft starts from work it has delivered.
func profileSuggestHandler(svc Workspace, wins WinSource, logger *zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		actor := auth.UserID(c)
		if actor == "" {
			return writeError(c, fiber.StatusUnauthorized, "user_required", "the request names no user")
		}
		if tooLarge(c) {
			return writeError(c, fiber.StatusRequestEntityTooLarge, "payload_too_large", "the request body is too large")
		}
		var req struct {
			OrgNumber string `json:"org_number"`
		}
		if err := json.Unmarshal(c.BodyRaw(), &req); err != nil {
			return writeError(c, fiber.StatusBadRequest, "bad_request", "send the organisation number as org_number")
		}
		orgNumber, err := normalize.OrgNumber(req.OrgNumber)
		if err != nil {
			return writeError(c, fiber.StatusBadRequest, "invalid_request", "the organisation number is not valid")
		}
		awards, err := wins.Awards(c.UserContext())
		if err != nil {
			return workspaceError(c, err, logger)
		}
		suggestion := profilesuggest.FromWins(awards, orgNumber)
		if suggestion.Wins == 0 {
			return c.JSON(fiber.Map{"data": fiber.Map{"applied": false, "reason": "no_wins", "suggestion": suggestion}})
		}
		raw, err := json.Marshal(suggestion.Profile("balanced"))
		if err != nil {
			return workspaceError(c, err, logger)
		}
		saved, created, err := svc.PutProfileIfAbsent(c.UserContext(), auth.OrganizationID(c), actor, raw)
		if err != nil {
			return workspaceError(c, err, logger)
		}
		if !created {
			return c.JSON(fiber.Map{"data": fiber.Map{"applied": false, "reason": "profile_saved", "suggestion": suggestion}})
		}
		return c.JSON(fiber.Map{"data": fiber.Map{"applied": true, "profile": saved, "suggestion": suggestion}})
	}
}

func pipelineReadHandler(svc Workspace, logger *zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		items, err := svc.ListPipeline(c.UserContext(), auth.OrganizationID(c))
		if err != nil {
			return workspaceError(c, err, logger)
		}
		return c.JSON(fiber.Map{"data": fiber.Map{"items": items}})
	}
}

func pipelineWriteHandler(svc Workspace, logger *zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		actor := auth.UserID(c)
		if actor == "" {
			return writeError(c, fiber.StatusUnauthorized, "user_required", "the request names no user")
		}
		if tooLarge(c) {
			return writeError(c, fiber.StatusRequestEntityTooLarge, "payload_too_large", "the request body is too large")
		}
		var req struct {
			NoticeID string `json:"notice_id"`
			Stage    string `json:"stage"`
		}
		if err := json.Unmarshal(c.BodyRaw(), &req); err != nil {
			return writeError(c, fiber.StatusBadRequest, "bad_request", "send notice_id and stage")
		}
		item, err := svc.SetStage(c.UserContext(), auth.OrganizationID(c), actor, req.NoticeID, req.Stage)
		if err != nil {
			return workspaceError(c, err, logger)
		}
		return c.JSON(fiber.Map{"data": item})
	}
}

func searchesReadHandler(svc Workspace, logger *zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		list, err := svc.ListSearches(c.UserContext(), auth.OrganizationID(c))
		if err != nil {
			return workspaceError(c, err, logger)
		}
		return c.JSON(fiber.Map{"data": fiber.Map{"searches": list}})
	}
}

func searchWriteHandler(svc Workspace, logger *zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		actor := auth.UserID(c)
		if actor == "" {
			return writeError(c, fiber.StatusUnauthorized, "user_required", "the request names no user")
		}
		if tooLarge(c) {
			return writeError(c, fiber.StatusRequestEntityTooLarge, "payload_too_large", "the request body is too large")
		}
		var req struct {
			Name  string          `json:"name"`
			Query json.RawMessage `json:"query"`
		}
		if err := json.Unmarshal(c.BodyRaw(), &req); err != nil || len(req.Query) == 0 {
			return writeError(c, fiber.StatusBadRequest, "bad_request", "send a name and the search as a JSON object in query")
		}
		saved, err := svc.SaveSearch(c.UserContext(), auth.OrganizationID(c), actor, req.Name, req.Query)
		if err != nil {
			return workspaceError(c, err, logger)
		}
		return c.JSON(fiber.Map{"data": saved})
	}
}

func searchDeleteHandler(svc Workspace, logger *zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		if auth.UserID(c) == "" {
			return writeError(c, fiber.StatusUnauthorized, "user_required", "the request names no user")
		}
		if tooLarge(c) {
			return writeError(c, fiber.StatusRequestEntityTooLarge, "payload_too_large", "the request body is too large")
		}
		var req struct {
			ID int64 `json:"id"`
		}
		if err := json.Unmarshal(c.BodyRaw(), &req); err != nil || req.ID <= 0 {
			return writeError(c, fiber.StatusBadRequest, "bad_request", "send the id of the saved search")
		}
		if err := svc.DeleteSearch(c.UserContext(), auth.OrganizationID(c), req.ID); err != nil {
			return workspaceError(c, err, logger)
		}
		return c.JSON(fiber.Map{"data": fiber.Map{"deleted": true}})
	}
}

const workspaceInvalidPrefix = "workspace: invalid value: "

// workspaceError maps the store's errors to the API's. The messages name a rule, never a value.
func workspaceError(c *fiber.Ctx, err error, logger *zerolog.Logger) error {
	switch {
	case errors.Is(err, workspace.ErrNotFound):
		return writeError(c, fiber.StatusNotFound, "not_found", "nothing of that id is saved for this organisation")
	case errors.Is(err, workspace.ErrTooMany):
		return writeError(c, fiber.StatusConflict, "limit_reached", "the organisation keeps at most 50 saved searches; delete one first")
	case errors.Is(err, workspace.ErrInvalid):
		return writeError(c, fiber.StatusBadRequest, "invalid_request", strings.TrimPrefix(err.Error(), workspaceInvalidPrefix))
	}
	if logger != nil {
		logger.Error().Err(err).Msg("workspace: request failed")
	}
	return writeError(c, fiber.StatusInternalServerError, "internal_error", "the workspace could not be read or saved")
}

// seenReadHandler answers when the organisation last marked its tenders as seen, or null when it never has.
func seenReadHandler(svc Workspace, logger *zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		at, ok, err := svc.Seen(c.UserContext(), auth.OrganizationID(c))
		if err != nil {
			return workspaceError(c, err, logger)
		}
		if !ok {
			return c.JSON(fiber.Map{"data": fiber.Map{"seen_at": nil}})
		}
		return c.JSON(fiber.Map{"data": fiber.Map{"seen_at": at.UTC().Format(time.RFC3339Nano)}})
	}
}

// seenWriteHandler records that the organisation has now looked at its tenders, and answers the instant.
func seenWriteHandler(svc Workspace, logger *zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		actor := auth.UserID(c)
		if actor == "" {
			return writeError(c, fiber.StatusUnauthorized, "user_required", "the request names no user")
		}
		at, err := svc.MarkSeen(c.UserContext(), auth.OrganizationID(c), actor)
		if err != nil {
			return workspaceError(c, err, logger)
		}
		return c.JSON(fiber.Map{"data": fiber.Map{"seen_at": at.UTC().Format(time.RFC3339Nano)}})
	}
}
