// quarry-control-go — canonical operator/control API.
//
// Owns: jobs, stores, snapshots, artifacts, browser profiles, sources,
// benchmarks, list/get/history/filter. GraphQL overlay optional.
package main

import (
	"context"
	"errors"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/quarry-v2/pkg/quarryotel"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/dispatcher"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/resources"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store/pg"
)

func main() {
	zerolog.TimeFieldFormat = zerolog.TimeFormatUnixMs
	log.Logger = log.Output(zerolog.ConsoleWriter{Out: os.Stderr, TimeFormat: time.RFC3339})

	otelCtx, otelCancel := context.WithCancel(context.Background())
	defer otelCancel()
	otelShutdown, err := quarryotel.Init(otelCtx, "quarry-control", "0.1.0")
	if err != nil {
		log.Warn().Err(err).Msg("OTEL init failed; continuing without tracing")
	}
	defer func() {
		if err := otelShutdown(context.Background()); err != nil {
			log.Warn().Err(err).Msg("OTEL shutdown failed")
		}
	}()

	addr := envOr("QUARRY_CONTROL_ADDR", ":8081")
	dsn := os.Getenv("QUARRY_CONTROL_DSN")
	apiKey := os.Getenv("QUARRY_CONTROL_API_KEY")
	if apiKey == "" {
		log.Warn().Msg("QUARRY_CONTROL_API_KEY empty — event ingestion endpoint will reject all requests")
	}
	// D2 / cluster #14 — HMAC cross-plane auth. When the secret is
	// unset, the verifier degrades to "trust the network"; once it's
	// set, we run in require=false mode for the rollout window so
	// existing callers that don't yet sign aren't broken. Flip
	// QUARRY_INTERNAL_HMAC_REQUIRED=1 to switch to enforce mode after
	// every edge instance is signing.
	internalSecret := os.Getenv("QUARRY_INTERNAL_SECRET")
	internalRequire := os.Getenv("QUARRY_INTERNAL_HMAC_REQUIRED") == "1"
	hmacVerifier := httpx.NewHMACVerifier(internalSecret, internalRequire)
	switch {
	case hmacVerifier.HasSecret() && internalRequire:
		log.Info().Msg("HMAC cross-plane auth: ENFORCED (every internal request must sign)")
	case hmacVerifier.HasSecret():
		log.Info().Msg("HMAC cross-plane auth: rollout mode (signatures verified when present)")
	default:
		log.Warn().Msg("HMAC cross-plane auth: DISABLED (no secret) — control plane MUST be on private network")
	}

	var db store.DB
	if dsn == "" {
		log.Warn().Msg("QUARRY_CONTROL_DSN empty — using in-memory store (dev only, non-durable)")
		db = store.NewMemory()
	} else {
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		pgDB, err := pg.New(ctx, dsn)
		if err != nil {
			log.Fatal().Err(err).Msg("postgres init")
		}
		log.Info().Msg("postgres store ready")
		db = pgDB
	}

	r := chi.NewRouter()
	r.Use(httpx.RequestID, httpx.Logger, httpx.Recover)

	r.Get("/health", httpx.Health)
	r.Get("/ready", httpx.Ready)

	// Internal API routes — every /v1/* path expects HMAC-signed
	// requests once the secret is set. /health and /ready stay
	// outside the middleware so probes work without secrets.
	r.Group(func(r chi.Router) {
		r.Use(hmacVerifier.Middleware)

		resources.MountJobs(r, db)
		resources.MountStores(r, db)
		resources.MountSnapshots(r, db)
		resources.MountArtifacts(r, db)
		resources.MountProfiles(r, db)
		resources.MountSchedules(r, db)
		resources.MountEvents(r, db, apiKey)
		resources.MountWebhooks(r, db)
		resources.MountBlocklists(r, db)
		resources.MountWebhookDeliveries(r, db)
		resources.MountPresets(r)

		// Cycle 23 additions — REST resource breadth part 2 +
		// schedule lifecycle aliases. Empty / stub implementations
		// where the underlying schema isn't ready yet; the contract
		// is in place so the Rust edge forwards work end-to-end.
		resources.MountSources(r)
		resources.MountBenchmarks(r)
		resources.MountRequestQueues(r, db)
		resources.MountTeam(r, db)
		resources.MountScheduleAliases(r, db)
		resources.MountJobsByKind(r, db)
	})

	dCtx, dCancel := context.WithCancel(context.Background())
	go dispatcher.Run(dCtx, db, nil, dispatcher.Options{Workers: 4}, log.Logger)

	srv := &http.Server{
		Addr:              addr,
		Handler:           r,
		ReadHeaderTimeout: 5 * time.Second,
	}
	go func() {
		log.Info().Str("addr", addr).Msg("quarry-control-go listening")
		if err := srv.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
			log.Fatal().Err(err).Msg("listen")
		}
	}()

	sig := make(chan os.Signal, 1)
	signal.Notify(sig, syscall.SIGINT, syscall.SIGTERM)
	<-sig

	dCancel()

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	_ = srv.Shutdown(ctx)
	log.Info().Msg("shutdown complete")
}

func envOr(k, d string) string {
	if v := os.Getenv(k); v != "" {
		return v
	}
	return d
}
