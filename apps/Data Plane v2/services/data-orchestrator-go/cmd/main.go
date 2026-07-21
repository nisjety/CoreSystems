package main

import (
	"context"
	"encoding/json"
	"errors"
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

	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/authctx"
	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/config"
	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/cost"
	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/gdpr"
	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/handler"
	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/jobs"
	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/metrics"
	apmotel "github.com/triodelab/dataplane/services/data-orchestrator-go/internal/otel"
)

func main() {
	zerolog.TimeFieldFormat = zerolog.TimeFormatUnix
	log.Logger = zerolog.New(os.Stdout).With().Timestamp().Str("service", "data-orchestrator-go").Logger()

	cfg := config.Load()
	if err := cfg.ValidateSignedCostEvents(); err != nil {
		log.Fatal().Err(err).Msg("signed cost event verification configuration invalid")
	}
	if err := cfg.ValidateGDPRConsumer(); err != nil {
		log.Fatal().Err(err).Msg("GDPR org-purge consumer configuration invalid")
	}
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audience:      cfg.JWTAudience,
		Issuer:        cfg.JWTIssuer,
		PublicKeyFile: cfg.JWTPublicKeyFile,
		JWKSURL:       cfg.JWKSURL,
	})
	if err != nil {
		log.Fatal().Err(err).Msg("JWT verification configuration invalid")
	}
	authMiddleware := authctx.Middleware(verifier)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	otelShutdown, err := apmotel.Init(ctx, "data-orchestrator-go")
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

	legacyEventsEnabled := unverifiedLegacyEventsEnabled()
	if err := validateCostConsumerMode(cfg.SignedCostEventsEnabled, legacyEventsEnabled); err != nil {
		log.Fatal().Err(err).Msg("cost event consumer mode invalid")
	}
	var nc *nats.Conn
	if cfg.SignedCostEventsEnabled || legacyEventsEnabled {
		natsOptions := []nats.Option{nats.Name("data-orchestrator")}
		if cfg.NatsToken != "" {
			natsOptions = append(natsOptions, nats.Token(cfg.NatsToken))
		}
		nc, err = nats.Connect(cfg.NatsURL, natsOptions...)
		if err != nil {
			log.Fatal().Err(err).Msg("nats connect failed")
		}
		defer nc.Close()
	}

	executor := jobs.NewExecutor(pool, nc, legacyEventsEnabled)
	staleDetector := jobs.NewStaleDetector(pool)
	orchHandler := handler.NewOrchestratorHandler(executor, staleDetector)

	// GDPR org-erasure durable consumer. Deliberately a SEPARATE NATS
	// connection from nc above: nc is this service's plane-local Data Plane
	// v2 broker connection (cost ledger, reindex jobs); this is a dedicated
	// identity ("data-orchestrator-gdpr") on the cross-plane SHARED broker
	// (control-shared-nats), scoped to exactly one pre-provisioned durable.
	// It has no stream or consumer administration rights and no legacy
	// token fallback — see internal/gdpr's package doc.
	var orgPurgeConsumer *gdpr.Consumer
	if cfg.SharedNatsURL != "" {
		sharedNc, sharedErr := nats.Connect(
			cfg.SharedNatsURL,
			nats.Name("data-orchestrator-gdpr-durable"),
			nats.UserInfo(cfg.SharedNatsUser, cfg.SharedNatsPassword),
			nats.CustomInboxPrefix("_INBOX.DATA_ORCHESTRATOR_GDPR"),
		)
		if sharedErr != nil {
			if cfg.GDPROrgPurgeConsumerRequired {
				log.Fatal().Err(sharedErr).Msg("required scoped GDPR org-purge NATS connection failed")
			}
			log.Warn().Err(sharedErr).Msg("optional scoped GDPR org-purge NATS connection failed")
		} else {
			defer sharedNc.Close()
			purgeRepo := gdpr.NewPurgeRepo(pool)
			orgPurgeConsumer, sharedErr = gdpr.StartOrgPurgeSubscriber(sharedNc, purgeRepo)
			if sharedErr != nil {
				if cfg.GDPROrgPurgeConsumerRequired {
					log.Fatal().Err(sharedErr).Msg("required durable GDPR org-purge consumer failed to bind")
				}
				log.Warn().Err(sharedErr).Msg("optional durable GDPR org-purge consumer failed to bind")
			} else {
				defer orgPurgeConsumer.Close() //nolint:errcheck
			}
		}
	} else if cfg.GDPROrgPurgeConsumerRequired {
		log.Fatal().Msg("required durable GDPR org-purge consumer is not configured")
	}

	if cfg.SignedCostEventsEnabled {
		registry, err := cost.LoadVerifierRegistryFromFiles(
			cfg.EmbeddingEventPublicKeyPath,
			cfg.RetrievalEventPublicKeyPath,
			cost.DefaultReplayCapacity,
		)
		if err != nil {
			log.Fatal().Err(err).Msg("signed cost event producer registry invalid")
		}
		costConsumer := cost.NewSignedConsumer(pool, nc, registry)
		cleanup, err := costConsumer.Start(ctx)
		if err != nil {
			log.Fatal().Err(err).Msg("signed cost ledger consumer failed to start")
		}
		defer cleanup()
		log.Info().Msg("producer-scoped signed cost ledger consumer enabled")
	} else if legacyEventsEnabled {
		log.Warn().Msg("unsigned cost ledger consumer enabled for insecure development")
		costConsumer := cost.NewLegacyConsumer(pool, nc)
		if cleanup, err := costConsumer.Start(ctx); err != nil {
			log.Warn().Err(err).Msg("cost ledger consumer failed to start; continuing without")
		} else {
			defer cleanup()
		}
	} else {
		log.Info().Msg("cost ledger consumer disabled")
	}

	r := chi.NewRouter()
	r.Use(chimw.RequestID)
	r.Use(chimw.RealIP)
	r.Use(chimw.Recoverer)
	r.Use(chimw.Timeout(60 * time.Second))
	r.Use(metrics.Middleware)

	r.Get("/health", handler.Health)
	r.Get("/readyz", func(w http.ResponseWriter, request *http.Request) {
		if cfg.GDPROrgPurgeConsumerRequired && orgPurgeConsumer == nil {
			http.Error(w, "required GDPR org-purge consumer unavailable", http.StatusServiceUnavailable)
			return
		}
		handler.Readyz(w, request)
	})
	r.Get("/internal/gdpr/org-purge/health", func(w http.ResponseWriter, request *http.Request) {
		snapshot := gdpr.ConsumerHealthSnapshot{Status: "disabled"}
		if orgPurgeConsumer != nil {
			healthContext, healthCancel := context.WithTimeout(request.Context(), 2*time.Second)
			defer healthCancel()
			snapshot = orgPurgeConsumer.Health(healthContext)
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{"data": snapshot})
	})
	r.Method("GET", "/metrics", metrics.Handler())

	handler.MountProtectedRoutes(r, authMiddleware, orchHandler)

	addr := fmt.Sprintf("0.0.0.0:%d", cfg.HTTPPort)
	srv := &http.Server{Addr: addr, Handler: r}

	go func() {
		log.Info().Str("addr", addr).Msg("data-orchestrator-go starting")
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
		os.Getenv("ALLOW_INSECURE_DEV_DEFAULTS") == "1" &&
		os.Getenv("ISOLATED_E2E") == "1"
}

func validateCostConsumerMode(signed, legacy bool) error {
	if signed && legacy {
		return errors.New("signed and unverified legacy cost consumers are mutually exclusive")
	}
	return nil
}
