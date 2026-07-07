package main

import (
	"context"
	"crypto/subtle"
	"fmt"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/go-chi/chi/v5"
	chimw "github.com/go-chi/chi/v5/middleware"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/config"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/events"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/gdpr"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/handler"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/metrics"
	apmotel "github.com/triodelab/dataplane/services/documents-api-go/internal/otel"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/repo"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/userauthz"
	"github.com/triodelab/dataplane/services/documents-api-go/pkg/authctx"
	"github.com/triodelab/dataplane/services/documents-api-go/pkg/usagepub"
)

func main() {
	zerolog.TimeFieldFormat = zerolog.TimeFormatUnix
	log.Logger = zerolog.New(os.Stdout).With().Timestamp().Str("service", "documents-api-go").Logger()

	cfg, err := config.Load()
	if err != nil {
		log.Fatal().Err(err).Msg("config load failed")
	}
	if cfg.InternalAPIKey == "" {
		// Fail closed: an empty key previously let the auth middleware wave every
		// request through (fail-open). Refuse to start unless an operator has
		// explicitly opted into the insecure local mode.
		if os.Getenv("ALLOW_INSECURE_DEV_DEFAULTS") == "1" {
			log.Warn().Msg("INTERNAL_API_KEY is empty and ALLOW_INSECURE_DEV_DEFAULTS=1 — documents API is UNAUTHENTICATED (local dev only)")
		} else {
			log.Fatal().Msg("INTERNAL_API_KEY is required (set ALLOW_INSECURE_DEV_DEFAULTS=1 to run unauthenticated locally)")
		}
	}

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	otelShutdown, err := apmotel.Init(ctx, "documents-api-go")
	if err != nil {
		log.Warn().Err(err).Msg("otel init failed, continuing without tracing")
	} else {
		defer otelShutdown(ctx)
	}

	pool, err := pgxpool.New(ctx, cfg.DatabaseURL)
	if err != nil {
		log.Fatal().Err(err).Msg("postgres connect failed")
	}
	defer pool.Close()

	if err := pool.Ping(ctx); err != nil {
		log.Fatal().Err(err).Msg("postgres ping failed")
	}

	nc, err := nats.Connect(cfg.NatsURL)
	if err != nil {
		log.Fatal().Err(err).Str("url", cfg.NatsURL).Msg("nats connect failed")
	}
	defer nc.Close()

	docRepo := repo.NewDocumentRepo(pool)
	sourceObjectRepo := repo.NewSourceObjectRepo(pool)
	publisher := events.NewPublisher(nc)

	// Per-User Data Ownership (GDPR): subscribe to the cross-plane erasure
	// fan-out on the SHARED bus and transfer an erased user's owned documents to
	// the org system account (emitting velion.gdpr.ownership.transferred). The
	// fan-out is published by user-core on velion-nats, not the Data-Plane bus, so
	// this uses a separate shared connection. Best-effort: a missing shared bus
	// just means the transfer doesn't run here (logged), never a startup failure.
	if cfg.SharedNatsURL != "" {
		sharedOpts := []nats.Option{nats.Name("documents-api-gdpr-sub")}
		if cfg.SharedNatsToken != "" {
			sharedOpts = append(sharedOpts, nats.Token(cfg.SharedNatsToken))
		}
		if sharedNc, sErr := nats.Connect(cfg.SharedNatsURL, sharedOpts...); sErr != nil {
			log.Warn().Err(sErr).Str("url", cfg.SharedNatsURL).Msg("shared NATS connect failed; GDPR ownership-transfer subscriber disabled")
		} else {
			defer sharedNc.Close()
			if subErr := gdpr.StartSubscriber(sharedNc, docRepo); subErr != nil {
				log.Warn().Err(subErr).Msg("GDPR ownership-transfer subscriber failed to start")
			}
		}
	} else {
		log.Warn().Msg("NATS_SHARED_URL unset; GDPR ownership-transfer subscriber disabled")
	}
	// Phase A · A1.5 — usage + audit publisher. Logs-only on connect
	// failure (the existing nc above is already required, so failure
	// here is structurally unreachable). The handler can keep a pointer
	// and emit fire-and-forget events without ever blocking the request.
	usagePublisher := usagepub.New(nc, "data-plane")
	_ = usagePublisher // wired into handler in a follow-up commit; the
	// publisher is created here so the wiring is reviewable today even
	// though no call site forwards it yet.
	// Per-user authz facade client (user-core). Resolves a viewer's explicit
	// document grants so List/Get can enforce ownership at the source.
	authzClient := userauthz.New(cfg.UserCoreURL, cfg.InternalAPIKey)
	docHandler := handler.NewDocumentHandler(docRepo, publisher, authzClient)
	sourceObjectHandler := handler.NewSourceObjectHandler(sourceObjectRepo, docRepo)

	// §16.2.6 — start outbox publisher loop. Drains `documents_outbox`
	// every 500ms with FOR UPDATE SKIP LOCKED so multiple replicas don't
	// double-publish. Honors `ctx.Done()` for graceful shutdown.
	events.NewOutboxPublisher(pool, nc).Start(ctx)

	// NOTE: the legacy shared-NATS "quarry.documents.crawled" subscriber was
	// removed. Quarry-v2 never published that subject (it emits quarry.run.* /
	// quarry.events.* for observability only); Quarry document ingestion is now
	// the canonical synchronous HTTP write to POST /v1/documents, and retrieval
	// readiness is signalled by dataplane.documents.indexed (embedding-engine).
	// See docs/plans/quarry-dataplane-integration-fix-plan.md.

	r := chi.NewRouter()
	r.Use(chimw.RequestID)
	r.Use(chimw.RealIP)
	r.Use(chimw.Recoverer)
	r.Use(chimw.Timeout(30 * time.Second))
	r.Use(metrics.Middleware)

	r.Get("/health", handler.Health)
	r.Get("/readyz", handler.Readyz)
	r.Method("GET", "/metrics", metrics.Handler())

	// Phase A · A1.2 — observe-mode authctx middleware. Decodes the
	// auth-core JWT (unverified, observe-only) and stuffs `Claims` into
	// the request context. Once every velion call site mints an
	// audience-scoped JWT we flip AUTHCTX_ENFORCE=1 — at that point
	// signature verification kicks in and the legacy X-Org-ID header
	// stops being trusted.
	authctxMiddleware := authctx.Middleware(authctx.Config{
		Audience:       "data-plane",
		JWKSURL:        envOrDefault("AUTH_CORE_JWKS_URL", "http://auth-core:3011/api/convex-auth/jwks"),
		ExpectedIssuer: envOrDefault("AUTH_CORE_ISSUER", "http://auth-core:3011/api/convex-auth"),
	})

	r.Route("/v1/documents", func(r chi.Router) {
		r.Use(internalAuthMiddleware(cfg.InternalAPIKey))
		r.Use(authctxMiddleware)
		r.Use(handler.OrgIDMiddleware)
		r.Get("/", docHandler.List)
		r.Post("/", docHandler.Create)
		r.Post("/bulk", docHandler.BulkIngest)
		r.Get("/{documentID}", docHandler.Get)
		r.Delete("/{documentID}", docHandler.Delete)
	})

	// U1-2 (velion ui-ux-velion-gap.md §10): distinct sources facet for the
	// velion dashboard's "Sources" stat. Quarry-v2 writes scrapes into
	// `documents` with their source URL; the answer to "how many sources
	// do I have" is COUNT(DISTINCT source) here, not anywhere in Quarry.
	r.Route("/v1/sources", func(r chi.Router) {
		r.Use(internalAuthMiddleware(cfg.InternalAPIKey))
		r.Use(authctxMiddleware)
		r.Use(handler.OrgIDMiddleware)
		r.Get("/", docHandler.Sources)
	})

	r.Route("/v1/source-objects", func(r chi.Router) {
		r.Use(internalAuthMiddleware(cfg.InternalAPIKey))
		r.Use(authctxMiddleware)
		r.Use(handler.OrgIDMiddleware)
		r.Get("/duplicates", sourceObjectHandler.Duplicates)
		r.Post("/", sourceObjectHandler.Upsert)
		r.Post("/delete", sourceObjectHandler.Delete)
	})

	addr := fmt.Sprintf("0.0.0.0:%d", cfg.HTTPPort)
	srv := &http.Server{Addr: addr, Handler: r}

	go func() {
		log.Info().Str("addr", addr).Msg("documents-api-go starting")
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Fatal().Err(err).Msg("http server error")
		}
	}()

	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	<-quit

	log.Info().Msg("shutting down")
	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer shutdownCancel()
	srv.Shutdown(shutdownCtx)
}

// envOrDefault reads an environment variable or returns the supplied
// fallback. Used to wire the Phase A · A1.2 authctx middleware without
// dragging an extra config-struct field through the codebase before the
// shape is finalised.
func envOrDefault(name, fallback string) string {
	v := os.Getenv(name)
	if v == "" {
		return fallback
	}
	return v
}

func internalAuthMiddleware(expected string) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if expected == "" {
				next.ServeHTTP(w, r)
				return
			}

			provided := r.Header.Get("X-Internal-Api-Key")
			if provided == "" {
				provided = r.Header.Get("X-Internal-Key")
			}
			if provided == "" {
				provided = r.Header.Get("X-Api-Key")
			}

			if subtle.ConstantTimeCompare([]byte(provided), []byte(expected)) != 1 {
				w.Header().Set("Content-Type", "application/json")
				w.WriteHeader(http.StatusUnauthorized)
				_, _ = w.Write([]byte(`{"error":"unauthorized"}`))
				return
			}

			next.ServeHTTP(w, r)
		})
	}
}
