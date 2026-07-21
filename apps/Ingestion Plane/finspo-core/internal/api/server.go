package api

import (
	"context"
	"errors"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/rs/zerolog"

	"github.com/triodelab/finspo/internal/auth"
	"github.com/triodelab/finspo/internal/events"
	"github.com/triodelab/finspo/internal/sharepoint"
)

type ServerConfig struct {
	APIKey       string
	APIKeyHeader string
	Browser      sharepoint.Browser

	// Optional collaborators. When nil, the affected endpoints degrade
	// gracefully (e.g. /ready reports the missing component as "skipped").
	Pool      *pgxpool.Pool
	Publisher *events.Publisher
	Logger    *zerolog.Logger

	// Phase 2 — admin/source endpoints. Wire from main.go once the store and
	// sync engine are constructed. Nil values cause those routes to be omitted.
	SourceReader SourceStoreReader
	SourceWriter SourceStoreWriter
	CursorReader CursorReader
	SyncRunner   SyncRunner

	// Phase 4 — governance surface: analytics + review proposals + audit.
	// Each is independently optional so a deployment can disable the
	// governance UI by leaving these nil.
	Analytics AnalyticsReader
	Proposals ProposalStore
	Audit     AuditWriter

	// Phase 5 — recommendations + execution.
	Recommender Recommender
	Executor    ProposalExecutor
	Subjects    events.Subjects
}

func NewServer(cfg ServerConfig) *fiber.App {
	app := fiber.New(fiber.Config{
		DisableStartupMessage: true,
	})
	browser := cfg.Browser
	if browser == nil {
		browser = sharepoint.DisconnectedBrowser{}
	}

	if cfg.Logger != nil {
		app.Use(requestLogger(*cfg.Logger))
	}

	app.Get("/health", func(c *fiber.Ctx) error {
		return c.JSON(fiber.Map{
			"status":  "ok",
			"service": "finspo-core",
		})
	})

	app.Get("/ready", readinessHandler(cfg.Pool, cfg.Publisher))

	authMW := auth.Middleware(auth.Config{
		APIKey:       cfg.APIKey,
		APIKeyHeader: cfg.APIKeyHeader,
	})

	v1 := app.Group("/api/v1/sharepoint", authMW)

	admin := app.Group("/api/v1", authMW)
	registerSourceRoutes(admin, cfg.SourceReader, cfg.SourceWriter, cfg.CursorReader, cfg.SyncRunner)
	registerAnalyticsRoutes(admin, cfg.Analytics)
	registerRecommendationRoutes(admin, cfg.Recommender)

	var emitter *LifecycleEmitter
	if cfg.Publisher != nil {
		emitter = &LifecycleEmitter{Publisher: cfg.Publisher, Subjects: cfg.Subjects}
	}
	registerProposalRoutes(admin, cfg.Proposals, cfg.Audit, cfg.Executor, emitter, cfg.Subjects)

	v1.Get("/sites", func(c *fiber.Ctx) error {
		organizationID := auth.OrganizationID(c)
		sites, err := browser.ListSites(c.UserContext(), organizationID)
		if err != nil {
			return writeSharePointError(c, err)
		}

		return c.JSON(fiber.Map{
			"success": true,
			"data": fiber.Map{
				"count": len(sites),
				"sites": sites,
			},
		})
	})

	v1.Get("/sites/:siteID/drives", func(c *fiber.Ctx) error {
		siteID := strings.TrimSpace(c.Params("siteID"))
		if siteID == "" {
			return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
				"success": false,
				"error":   "siteID is required",
			})
		}

		organizationID := auth.OrganizationID(c)
		drives, err := browser.ListDrives(c.UserContext(), organizationID, siteID)
		if err != nil {
			return writeSharePointError(c, err)
		}

		return c.JSON(fiber.Map{
			"success": true,
			"data": fiber.Map{
				"site_id": siteID,
				"count":   len(drives),
				"drives":  drives,
			},
		})
	})

	v1.Get("/sites/:siteID/items", func(c *fiber.Ctx) error {
		siteID := strings.TrimSpace(c.Params("siteID"))
		if siteID == "" {
			return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
				"success": false,
				"error":   "siteID is required",
			})
		}

		path := strings.TrimSpace(c.Query("path", "/"))
		if path == "" {
			path = "/"
		}

		organizationID := auth.OrganizationID(c)
		items, err := browser.ListItems(c.UserContext(), organizationID, siteID, path)
		if err != nil {
			return writeSharePointError(c, err)
		}

		return c.JSON(fiber.Map{
			"success": true,
			"data": fiber.Map{
				"site_id": siteID,
				"path":    path,
				"count":   len(items),
				"items":   items,
			},
		})
	})

	return app
}

func writeSharePointError(c *fiber.Ctx, err error) error {
	status := fiber.StatusBadGateway
	message := "sharepoint request failed"
	if errors.Is(err, sharepoint.ErrNotConfigured) {
		status = fiber.StatusServiceUnavailable
		message = err.Error()
	}

	return c.Status(status).JSON(fiber.Map{
		"success": false,
		"error":   message,
	})
}

func readinessHandler(pool *pgxpool.Pool, publisher *events.Publisher) fiber.Handler {
	return func(c *fiber.Ctx) error {
		ctx, cancel := context.WithTimeout(c.UserContext(), 2*time.Second)
		defer cancel()

		checks := fiber.Map{}
		ok := true

		if pool == nil {
			checks["db"] = "skipped"
		} else if err := pool.Ping(ctx); err != nil {
			checks["db"] = "down: " + err.Error()
			ok = false
		} else {
			checks["db"] = "ok"
		}

		if publisher == nil {
			checks["nats"] = "skipped"
		} else if publisher.Healthy() {
			checks["nats"] = "ok"
		} else {
			checks["nats"] = "down"
			ok = false
		}

		status := fiber.StatusOK
		if !ok {
			status = fiber.StatusServiceUnavailable
		}
		return c.Status(status).JSON(fiber.Map{
			"status": map[bool]string{true: "ok", false: "degraded"}[ok],
			"checks": checks,
		})
	}
}

func requestLogger(logger zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		start := time.Now()
		err := c.Next()
		logger.Info().
			Str("method", c.Method()).
			Str("path", c.Path()).
			Int("status", c.Response().StatusCode()).
			Dur("duration", time.Since(start)).
			Msg("http_request")
		return err
	}
}
