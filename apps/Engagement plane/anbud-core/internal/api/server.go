// Package api serves anbud-core's HTTP surface.
//
// Responses use the monorepo envelopes: { "data": ... } for success and
// { "error": { "code", "message", "details"? } } for failure. /health and /ready are plain
// infrastructure endpoints and need no authentication.
package api

import (
	"context"
	"errors"
	"net/http"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/gofiber/fiber/v2/middleware/adaptor"
	fiberrecover "github.com/gofiber/fiber/v2/middleware/recover"
	"github.com/rs/zerolog"
	"github.com/triodelab/studio-plane/planeclient"

	"github.com/triodelab/anbud/internal/auth"
	"github.com/triodelab/anbud/internal/demo"
	"github.com/triodelab/anbud/internal/secret"
)

const serviceName = "anbud-core"

// Pinger is the part of the database pool that readiness needs.
type Pinger interface {
	Ping(ctx context.Context) error
}

// SourceStatus describes one upstream notice source. It never carries secrets.
type SourceStatus struct {
	ID         string         `json:"id"`
	Configured bool           `json:"configured"`
	Budgets    map[string]int `json:"budgets_per_minute,omitempty"`
}

// ServerConfig wires the server's collaborators. Nil collaborators degrade
// gracefully: readiness reports the database as "skipped".
type ServerConfig struct {
	APIKey       secret.Value
	APIKeyHeader string
	DB           Pinger
	Sources      func() []SourceStatus
	Logger       *zerolog.Logger
	// Demo is the logic behind the demo routes. Nil leaves the routes out: a service with no notice
	// store has nothing to show.
	Demo *demo.Service
	// Documents is the tender document service behind the upload routes. Nil leaves the routes out:
	// the feature is opt-in, because it keeps a company's documents (docs/PERSONAL-DATA.md).
	Documents Documents
	// Workspace keeps what an organisation does with the tender list: its profile, pipeline and saved searches.
	// Nil leaves the routes out.
	Workspace Workspace
	// Wins is where a suggested profile is drawn from. Nil leaves the suggest route out.
	Wins   WinSource
	Planes *planeclient.Client
}

// NewServer builds the HTTP application.
func NewServer(cfg ServerConfig) *fiber.App {
	app := fiber.New(fiber.Config{
		DisableStartupMessage: true,
		ErrorHandler:          errorHandler(cfg.Logger),
		// This service sits behind the gateway and takes small JSON bodies (the demo routes, which
		// refuse more than 64 KiB themselves) and, on one route, an archive: the header hook below
		// gives that route more room. Keep slow or oversized requests from tying up connections.
		ReadTimeout:  15 * time.Second,
		WriteTimeout: 30 * time.Second,
		IdleTimeout:  60 * time.Second,
		BodyLimit:    1 << 20,
	})
	// The access log sits outside recover, so a panic that is recovered into a
	// 500 is still logged with that status.
	if cfg.Logger != nil {
		app.Use(requestLogger(*cfg.Logger))
	}
	app.Use(fiberrecover.New())
	app.Use(refuseEncodedBodies())

	app.Get("/health", func(c *fiber.Ctx) error {
		return c.JSON(fiber.Map{"status": "ok", "service": serviceName})
	})
	app.Get("/ready", readinessHandler(cfg))

	// By default the server parses a multipart body before any handler sees it, and streams a large file
	// of it into a temporary file on disk. Nothing here takes a form, and an uploaded archive is read in
	// memory and written nowhere: a multipart body is refused by its content type and never parsed.
	app.Server().DisablePreParseMultipartForm = true

	// When the parser refuses a request, its error says why and, by default, quotes the first and the last two hundred
	// bytes of the head, which behind the gateway holds the key it adds to what it passes on. The error handler
	// does not pass that text on (publicMessage), and the server does not put it in the error in the first place,
	// so that no log or later change to the handler brings it back.
	app.Server().SecureErrorLogMessage = true

	authConfig := auth.Config{APIKey: cfg.APIKey, APIKeyHeader: cfg.APIKeyHeader}
	v1 := app.Group("/api/v1", auth.Middleware(authConfig))
	v1.Get("/sources", sourcesHandler(cfg.Sources))
	if cfg.Planes != nil {
		v1.Get("/anbud/plane-connections", adaptor.HTTPHandler(planeclient.ConnectionsHandler(cfg.Planes)))
		v1.Post("/anbud/assistant/drafts", adaptor.HTTPHandler(planeclient.DraftHandler(cfg.Planes)))
	}
	if cfg.Demo != nil {
		registerDemo(v1, cfg.Demo, cfg.Logger)
	}
	if cfg.Documents != nil {
		registerDocuments(v1, cfg.Documents)
		app.Server().HeaderReceived = uploadLimits(authConfig, cfg.Documents.MaxArchiveBytes())
	}
	if cfg.Workspace != nil {
		registerWorkspace(v1, cfg.Workspace, cfg.Wins, cfg.Logger)
	}

	return app
}

func sourcesHandler(provider func() []SourceStatus) fiber.Handler {
	return func(c *fiber.Ctx) error {
		sources := []SourceStatus{}
		if provider != nil {
			if s := provider(); s != nil {
				sources = s
			}
		}
		return c.JSON(fiber.Map{"data": fiber.Map{"sources": sources}})
	}
}

// readinessHandler reports whether the service can do its job. A source that
// is not configured does not make the service unready: it can still serve the
// corpus it already holds, and the state is visible under "sources".
func readinessHandler(cfg ServerConfig) fiber.Handler {
	return func(c *fiber.Ctx) error {
		ctx, cancel := context.WithTimeout(c.UserContext(), 2*time.Second)
		defer cancel()

		ready := true
		db := "skipped"
		if cfg.DB != nil {
			if err := cfg.DB.Ping(ctx); err != nil {
				db, ready = "down", false
				// The error can name the host and user; it goes to the log, not the response.
				if cfg.Logger != nil {
					cfg.Logger.Warn().Err(err).Msg("readiness: database ping failed")
				}
			} else {
				db = "ok"
			}
		}

		sources := map[string]string{}
		if cfg.Sources != nil {
			for _, s := range cfg.Sources() {
				state := "not_configured"
				if s.Configured {
					state = "configured"
				}
				sources[s.ID] = state
			}
		}

		status, label := fiber.StatusOK, "ok"
		if !ready {
			status, label = fiber.StatusServiceUnavailable, "degraded"
		}
		return c.Status(status).JSON(fiber.Map{
			"status":  label,
			"checks":  fiber.Map{"db": db},
			"sources": sources,
		})
	}
}

func errorHandler(logger *zerolog.Logger) fiber.ErrorHandler {
	return func(c *fiber.Ctx, err error) error {
		var fiberErr *fiber.Error
		if errors.As(err, &fiberErr) {
			return writeError(c, fiberErr.Code, errorCode(fiberErr.Code), publicMessage(fiberErr.Code))
		}
		if logger != nil {
			logger.Error().Err(err).Str("path", c.Path()).Msg("unhandled_error")
		}
		return writeError(c, fiber.StatusInternalServerError, "internal", "internal error")
	}
}

// publicMessage is what a client is told of an error the framework made, for the status it chose. The framework's own
// words are not used: for a request the HTTP parser refused they hold a piece of the request itself, the headers the
// gateway added among them, and the gateway passes the answer on to whoever sent the request.
func publicMessage(status int) string {
	switch status {
	case fiber.StatusBadRequest:
		return "the request could not be read"
	case fiber.StatusNotFound:
		return "route not found"
	}
	if text := http.StatusText(status); text != "" {
		return text
	}
	return "error"
}

func errorCode(status int) string {
	switch status {
	case fiber.StatusBadRequest:
		return "bad_request"
	case fiber.StatusUnauthorized:
		return "unauthorized"
	case fiber.StatusForbidden:
		return "forbidden"
	case fiber.StatusNotFound:
		return "not_found"
	case fiber.StatusMethodNotAllowed:
		return "method_not_allowed"
	case fiber.StatusRequestEntityTooLarge:
		return "payload_too_large"
	case fiber.StatusUnsupportedMediaType:
		return "unsupported_media_type"
	case fiber.StatusTooManyRequests:
		return "rate_limited"
	}
	if status >= 500 {
		return "internal"
	}
	return "error"
}

func writeError(c *fiber.Ctx, status int, code, message string) error {
	return c.Status(status).JSON(fiber.Map{
		"error": fiber.Map{"code": code, "message": message},
	})
}

// writeErrorDetails is writeError with the structured details the envelope allows: which fields
// of a request are wrong, or which shape a notice has.
func writeErrorDetails(c *fiber.Ctx, status int, code, message string, details any) error {
	return c.Status(status).JSON(fiber.Map{
		"error": fiber.Map{"code": code, "message": message, "details": details},
	})
}

func requestLogger(logger zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		start := time.Now()
		err := c.Next()
		if err != nil {
			// Fiber runs the ErrorHandler only after the whole chain has returned,
			// so the response would still say 200 here. Render the error now so
			// the logged status is the one the client actually receives.
			if handlerErr := c.App().Config().ErrorHandler(c, err); handlerErr != nil {
				_ = c.SendStatus(fiber.StatusInternalServerError)
			}
			err = nil
		}
		// Method and path only: query strings may carry search text.
		logger.Info().
			Str("method", c.Method()).
			Str("path", c.Path()).
			Int("status", c.Response().StatusCode()).
			Dur("duration", time.Since(start)).
			Msg("http_request")
		return err
	}
}
