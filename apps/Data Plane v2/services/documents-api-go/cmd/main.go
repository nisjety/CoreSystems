package main

import (
	"context"
	"encoding/json"
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
	"github.com/triodelab/dataplane/services/documents-api-go/internal/eventauth"
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
	if cfg.UserCoreServiceToken == "" {
		// This audience-bound credential is outbound-only for user-core grant
		// resolution. Missing grant authority must fail the
		// service closed instead of silently broadening document visibility.
		log.Fatal().Msg("USER_CORE_SERVICE_TOKEN is required for user-core grant resolution")
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

	natsOptions := []nats.Option{nats.Name("documents-api")}
	if cfg.NatsToken != "" {
		natsOptions = append(natsOptions, nats.Token(cfg.NatsToken))
	}
	nc, err := nats.Connect(cfg.NatsURL, natsOptions...)
	if err != nil {
		log.Fatal().Err(err).Msg("nats connect failed")
	}
	defer nc.Close()
	eventKey, err := os.ReadFile(cfg.EventSigningPrivateKeyPath)
	if err != nil {
		log.Fatal().Err(err).Msg("event signing key unavailable")
	}
	eventSigner, err := eventauth.NewSigner(
		eventKey,
		"service:documents-api-go",
		envOrDefault("EVENT_SIGNING_KEY_ID", "documents-events-v1"),
		envOrDefault("EVENT_AUTH_AUDIENCE", "dataplane-events"),
		"events:documents:publish",
	)
	if err != nil {
		log.Fatal().Err(err).Msg("event signing configuration invalid")
	}

	docRepo := repo.NewDocumentRepo(pool)
	sourceObjectRepo := repo.NewSourceObjectRepo(pool)

	// The shared broker identity is scoped to one pre-provisioned durable,
	// its ACK subject, ownership receipts, and a single DLQ. It has no stream or
	// consumer administration rights and no legacy token fallback.
	var gdprConsumer *gdpr.Consumer
	if cfg.SharedNatsURL != "" {
		sharedNc, sharedErr := nats.Connect(
			cfg.SharedNatsURL,
			nats.Name("documents-api-gdpr-durable"),
			nats.UserInfo(cfg.SharedNatsUser, cfg.SharedNatsPassword),
			nats.CustomInboxPrefix("_INBOX.DOCUMENTS_GDPR"),
		)
		if sharedErr != nil {
			if cfg.GDPRConsumerRequired {
				log.Fatal().Err(sharedErr).Msg("required scoped GDPR NATS connection failed")
			}
			log.Warn().Err(sharedErr).Msg("optional scoped GDPR NATS connection failed")
		} else {
			defer sharedNc.Close()
			gdprConsumer, sharedErr = gdpr.StartSubscriber(sharedNc, docRepo)
			if sharedErr != nil {
				if cfg.GDPRConsumerRequired {
					log.Fatal().Err(sharedErr).Msg("required durable GDPR consumer failed to bind")
				}
				log.Warn().Err(sharedErr).Msg("optional durable GDPR consumer failed to bind")
			} else {
				defer gdprConsumer.Close() //nolint:errcheck
			}
		}
	} else if cfg.GDPRConsumerRequired {
		log.Fatal().Msg("required durable GDPR consumer is not configured")
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
	authzClient := userauthz.New(cfg.UserCoreURL, cfg.UserCoreServiceToken)
	docHandler := handler.NewDocumentHandler(docRepo, authzClient)
	sourceObjectHandler := handler.NewSourceObjectHandler(sourceObjectRepo)

	// §16.2.6 — start outbox publisher loop. Drains `documents_outbox`
	// every 500ms with FOR UPDATE SKIP LOCKED so multiple replicas don't
	// double-publish. Honors `ctx.Done()` for graceful shutdown.
	outboxPublisher, err := events.NewOutboxPublisher(pool, nc, eventSigner)
	if err != nil {
		log.Fatal().Err(err).Msg("documents outbox publisher unavailable")
	}
	outboxPublisher.Start(ctx)

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
	r.Get("/readyz", func(w http.ResponseWriter, request *http.Request) {
		if cfg.GDPRConsumerRequired && gdprConsumer == nil {
			http.Error(w, "required GDPR consumer unavailable", http.StatusServiceUnavailable)
			return
		}
		handler.Readyz(w, request)
	})
	r.Get("/internal/gdpr/health", func(w http.ResponseWriter, request *http.Request) {
		snapshot := gdpr.ConsumerHealthSnapshot{Status: "disabled"}
		if gdprConsumer != nil {
			healthContext, healthCancel := context.WithTimeout(request.Context(), 2*time.Second)
			defer healthCancel()
			snapshot = gdprConsumer.Health(healthContext)
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{"data": snapshot})
	})
	r.Method("GET", "/metrics", metrics.Handler())

	authConfig := authctx.Config{
		Audience:       "data-plane",
		JWKSURL:        envOrDefault("AUTH_CORE_JWKS_URL", "http://auth-core:3011/api/convex-auth/jwks"),
		ExpectedIssuer: envOrDefault("AUTH_CORE_ISSUER", "http://auth-core:3011/api/convex-auth"),
	}
	if err := authctx.Validate(authConfig); err != nil {
		log.Fatal().Err(err).Msg("JWT verification configuration invalid")
	}
	authctxMiddleware := authctx.Middleware(authConfig)

	r.Route("/v1/documents", func(r chi.Router) {
		r.Use(authctxMiddleware)
		r.Use(handler.OrgIDMiddleware)
		read := r.With(authctx.RequireServiceScope("documents:read"))
		write := r.With(authctx.RequireServiceScope("documents:write"))
		read.Get("/", docHandler.List)
		write.Post("/", docHandler.Create)
		write.Post("/bulk", docHandler.BulkIngest)
		read.Get("/{documentID}", docHandler.Get)
		write.Delete("/{documentID}", docHandler.Delete)
	})

	// U1-2 (velion ui-ux-velion-gap.md §10): distinct sources facet for the
	// velion dashboard's "Sources" stat. Quarry-v2 writes scrapes into
	// `documents` with their source URL; the answer to "how many sources
	// do I have" is COUNT(DISTINCT source) here, not anywhere in Quarry.
	r.Route("/v1/sources", func(r chi.Router) {
		r.Use(authctxMiddleware)
		r.Use(handler.OrgIDMiddleware)
		r.With(authctx.RequireServiceScope("documents:read")).Get("/", docHandler.Sources)
	})

	r.Route("/v1/source-objects", func(r chi.Router) {
		r.Use(authctxMiddleware)
		r.Use(handler.OrgIDMiddleware)
		r.With(authctx.RequireServiceScope("documents:read")).Get("/duplicates", sourceObjectHandler.Duplicates)
		write := r.With(authctx.RequireServiceScope("documents:write"))
		write.Post("/", sourceObjectHandler.Upsert)
		write.Post("/delete", sourceObjectHandler.Delete)
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

func unverifiedLegacyEventsEnabled() bool {
	return os.Getenv("ALLOW_UNVERIFIED_LEGACY_EVENTS") == "1" &&
		os.Getenv("ALLOW_INSECURE_DEV_DEFAULTS") == "1"
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
