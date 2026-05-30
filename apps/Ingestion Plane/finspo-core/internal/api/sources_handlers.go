package api

import (
	"context"
	"errors"
	"strings"

	"github.com/gofiber/fiber/v2"
	"github.com/google/uuid"

	"github.com/triodelab/finspo/internal/auth"
	"github.com/triodelab/finspo/internal/store"
	"github.com/triodelab/finspo/internal/sync"
)

// SourceStoreReader/Writer are the narrow surfaces the source handlers need
// from the store. They are interfaces so server tests can swap in fakes.
type SourceStoreReader interface {
	Get(ctx context.Context, id uuid.UUID) (store.Source, error)
	ListByOrganization(ctx context.Context, organizationID string) ([]store.Source, error)
}

type SourceStoreWriter interface {
	EnsureSource(ctx context.Context, src store.Source) (store.Source, error)
}

type CursorReader interface {
	Get(ctx context.Context, sourceID uuid.UUID) (store.Cursor, error)
}

type SyncRunner interface {
	SyncDrive(ctx context.Context, sourceID uuid.UUID) (sync.SyncResult, error)
}

type sourceCreateRequest struct {
	TenantID   string `json:"tenant_id"`
	SiteID     string `json:"site_id"`
	SiteWebURL string `json:"site_web_url,omitempty"`
	DriveID    string `json:"drive_id"`
	DriveName  string `json:"drive_name,omitempty"`
	DriveType  string `json:"drive_type,omitempty"`
	Enabled    *bool  `json:"enabled,omitempty"`
}

func registerSourceRoutes(g fiber.Router, reader SourceStoreReader, writer SourceStoreWriter, cursors CursorReader, runner SyncRunner) {
	if reader == nil || writer == nil {
		// Phase 1 startup path without DB-backed handlers; skip silently.
		return
	}

	g.Post("/sources", createSourceHandler(reader, writer))
	g.Get("/sources", listSourcesHandler(reader))
	g.Get("/sources/:id", getSourceHandler(reader))
	g.Get("/sources/:id/status", getSourceStatusHandler(reader, cursors))
	g.Post("/sources/:id/sync", syncSourceHandler(reader, runner))
}

func createSourceHandler(reader SourceStoreReader, writer SourceStoreWriter) fiber.Handler {
	return func(c *fiber.Ctx) error {
		_ = reader // signature is symmetric; reader is unused on the create path
		var body sourceCreateRequest
		if err := c.BodyParser(&body); err != nil {
			return clientError(c, fiber.StatusBadRequest, "invalid JSON body")
		}
		body.SiteID = strings.TrimSpace(body.SiteID)
		body.DriveID = strings.TrimSpace(body.DriveID)
		if body.SiteID == "" || body.DriveID == "" {
			return clientError(c, fiber.StatusBadRequest, "site_id and drive_id are required")
		}

		enabled := true
		if body.Enabled != nil {
			enabled = *body.Enabled
		}

		src, err := writer.EnsureSource(c.UserContext(), store.Source{
			OrganizationID: auth.OrganizationID(c),
			TenantID:       strings.TrimSpace(body.TenantID),
			SiteID:         body.SiteID,
			SiteWebURL:     strings.TrimSpace(body.SiteWebURL),
			DriveID:        body.DriveID,
			DriveName:      strings.TrimSpace(body.DriveName),
			DriveType:      strings.TrimSpace(body.DriveType),
			Enabled:        enabled,
		})
		if err != nil {
			return serverError(c, "ensure source", err)
		}
		return c.Status(fiber.StatusCreated).JSON(fiber.Map{
			"success": true,
			"data":    src,
		})
	}
}

func listSourcesHandler(reader SourceStoreReader) fiber.Handler {
	return func(c *fiber.Ctx) error {
		sources, err := reader.ListByOrganization(c.UserContext(), auth.OrganizationID(c))
		if err != nil {
			return serverError(c, "list sources", err)
		}
		return c.JSON(fiber.Map{
			"success": true,
			"data": fiber.Map{
				"count":   len(sources),
				"sources": sources,
			},
		})
	}
}

func getSourceHandler(reader SourceStoreReader) fiber.Handler {
	return func(c *fiber.Ctx) error {
		id, err := uuid.Parse(c.Params("id"))
		if err != nil {
			return clientError(c, fiber.StatusBadRequest, "invalid source id")
		}
		src, err := reader.Get(c.UserContext(), id)
		if errors.Is(err, store.ErrNotFound) {
			return clientError(c, fiber.StatusNotFound, "source not found")
		}
		if err != nil {
			return serverError(c, "get source", err)
		}
		if src.OrganizationID != auth.OrganizationID(c) {
			return clientError(c, fiber.StatusNotFound, "source not found")
		}
		return c.JSON(fiber.Map{"success": true, "data": src})
	}
}

func getSourceStatusHandler(reader SourceStoreReader, cursors CursorReader) fiber.Handler {
	return func(c *fiber.Ctx) error {
		id, err := uuid.Parse(c.Params("id"))
		if err != nil {
			return clientError(c, fiber.StatusBadRequest, "invalid source id")
		}
		src, err := reader.Get(c.UserContext(), id)
		if errors.Is(err, store.ErrNotFound) {
			return clientError(c, fiber.StatusNotFound, "source not found")
		}
		if err != nil {
			return serverError(c, "get source", err)
		}
		if src.OrganizationID != auth.OrganizationID(c) {
			return clientError(c, fiber.StatusNotFound, "source not found")
		}
		cur, curErr := cursors.Get(c.UserContext(), id)
		if curErr != nil && !errors.Is(curErr, store.ErrNotFound) {
			return serverError(c, "get cursor", curErr)
		}
		return c.JSON(fiber.Map{
			"success": true,
			"data": fiber.Map{
				"source": src,
				"cursor": cur,
			},
		})
	}
}

func syncSourceHandler(reader SourceStoreReader, runner SyncRunner) fiber.Handler {
	return func(c *fiber.Ctx) error {
		if runner == nil {
			return clientError(c, fiber.StatusServiceUnavailable, "sync engine not configured")
		}
		id, err := uuid.Parse(c.Params("id"))
		if err != nil {
			return clientError(c, fiber.StatusBadRequest, "invalid source id")
		}
		src, err := reader.Get(c.UserContext(), id)
		if errors.Is(err, store.ErrNotFound) {
			return clientError(c, fiber.StatusNotFound, "source not found")
		}
		if err != nil {
			return serverError(c, "get source", err)
		}
		if src.OrganizationID != auth.OrganizationID(c) {
			return clientError(c, fiber.StatusNotFound, "source not found")
		}
		res, err := runner.SyncDrive(c.UserContext(), id)
		if err != nil {
			return serverError(c, "sync drive", err)
		}
		return c.JSON(fiber.Map{"success": true, "data": res})
	}
}

func clientError(c *fiber.Ctx, status int, msg string) error {
	return c.Status(status).JSON(fiber.Map{"success": false, "error": msg})
}

func serverError(c *fiber.Ctx, operation string, err error) error {
	return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
		"success": false,
		"error":   operation + " failed",
		"detail":  err.Error(),
	})
}
