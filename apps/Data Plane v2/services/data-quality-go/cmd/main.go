package main

import (
	"context"
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

	"github.com/triodelab/dataplane/services/data-quality-go/internal/authctx"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/config"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/cost"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/eval"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/gates"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/gdpr"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/handler"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/lint"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/metrics"
	apmotel "github.com/triodelab/dataplane/services/data-quality-go/internal/otel"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/trust"
)

func main() {
	zerolog.TimeFieldFormat = zerolog.TimeFormatUnix
	log.Logger = zerolog.New(os.Stdout).With().Timestamp().Str("service", "data-quality-go").Logger()

	cfg, err := config.Load()
	if err != nil {
		log.Fatal().Err(err).Msg("config invalid")
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

	otelShutdown, err := apmotel.Init(ctx, "data-quality-go")
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

	runner := eval.NewRunner(pool)
	go func() {
		const recoveryInterval = time.Minute
		const staleAfter = 5 * time.Minute
		for {
			if err := runner.Recover(ctx, staleAfter); err != nil {
				log.Error().Err(err).Msg("durable evaluation recovery failed")
			}
			select {
			case <-ctx.Done():
				return
			case <-time.After(recoveryInterval):
			}
		}
	}()
	scorer := trust.NewScorer(pool)
	checker := gates.NewChecker(pool)
	linter := lint.NewLinter(pool)
	costQuery := cost.NewQuery(pool)
	goldenStore := eval.NewPostgresGoldenStore(pool)
	qualityHandler := handler.NewQualityHandler(runner, goldenStore, scorer, checker, linter, costQuery)

	// Cross-plane GDPR erasure fan-out: org-core publishes
	// velion.gdpr.erasure.requested (explicit hard-delete AND its 30-day
	// auto-purge cron) on the shared Control-Plane bus; this hard-purges
	// quality_eval_runs and eval_golden_judgments for that org. Runs on a
	// SECOND, dedicated connection to the shared broker (control-shared-nats,
	// identity "data-quality-gdpr") — independent of this service's database
	// connection and never sharing plane-local broker traffic, since
	// data-quality-go has no plane-local NATS client of its own. Optional —
	// an unset NATS_SHARED_URL disables only this consumer, matching
	// GDPRConsumerRequired's fail-open default for local/dev.
	orgPurger := gdpr.NewPostgresOrgPurger(pool)
	var gdprConsumer *gdpr.Consumer
	if cfg.SharedNatsURL != "" {
		sharedNc, sharedErr := nats.Connect(
			cfg.SharedNatsURL,
			nats.Name("data-quality-gdpr"),
			nats.UserInfo(cfg.SharedNatsUser, cfg.SharedNatsPassword),
			nats.CustomInboxPrefix("_INBOX.DATA_QUALITY_GDPR"),
		)
		if sharedErr != nil {
			if cfg.GDPRConsumerRequired {
				log.Fatal().Err(sharedErr).Msg("required scoped GDPR NATS connection failed")
			}
			log.Warn().Err(sharedErr).Msg("optional scoped GDPR NATS connection failed; org-erasure consumer disabled")
		} else {
			defer sharedNc.Close()
			gdprConsumer, sharedErr = gdpr.Start(sharedNc, orgPurger)
			if sharedErr != nil {
				if cfg.GDPRConsumerRequired {
					log.Fatal().Err(sharedErr).Msg("required durable GDPR org-erasure consumer failed to bind")
				}
				log.Warn().Err(sharedErr).Msg("optional durable GDPR org-erasure consumer failed to bind")
			} else {
				defer gdprConsumer.Close() //nolint:errcheck
			}
		}
	} else if cfg.GDPRConsumerRequired {
		log.Fatal().Msg("required durable GDPR org-erasure consumer is not configured")
	} else {
		log.Warn().Msg("NATS_SHARED_URL unset — GDPR org-erasure consumer disabled")
	}

	r := chi.NewRouter()
	r.Use(chimw.RequestID)
	r.Use(chimw.RealIP)
	r.Use(chimw.Recoverer)
	r.Use(chimw.Timeout(120 * time.Second))
	r.Use(metrics.Middleware)

	r.Get("/health", handler.Health)
	r.Get("/readyz", handler.Readyz)
	r.Method("GET", "/metrics", metrics.Handler())

	handler.MountProtectedRoutes(r, authMiddleware, qualityHandler)

	addr := fmt.Sprintf("0.0.0.0:%d", cfg.HTTPPort)
	srv := &http.Server{Addr: addr, Handler: r}

	go func() {
		log.Info().Str("addr", addr).Msg("data-quality-go starting")
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
