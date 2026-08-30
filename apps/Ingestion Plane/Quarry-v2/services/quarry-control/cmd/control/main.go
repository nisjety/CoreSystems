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
	"strings"
	"syscall"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/quarry-v2/pkg/quarryotel"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/dispatcher"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/gdpr"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/janitor"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/notify"
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

	// W2: the one in-product notification on a detected change is delivered
	// to notification-core (Application Plane) over HTTP. Unset URL → the
	// notify leg is inert (nil sink); change tracking + webhooks still work.
	var notifySink notify.Sink
	if u := strings.TrimSpace(os.Getenv("NOTIFICATION_CORE_URL")); u != "" {
		notifySink = notify.NewHTTPSink(u, os.Getenv("NOTIFICATION_CORE_INTERNAL_KEY"))
		log.Info().Str("url", u).Msg("notification-core sink wired for change notifications")
	} else {
		log.Warn().Msg("NOTIFICATION_CORE_URL unset — change notifications disabled (change tracking + webhooks unaffected)")
	}
	envName := strings.ToLower(strings.TrimSpace(os.Getenv("ENVIRONMENT")))
	isProd := envName == "prod" || envName == "production"

	// Production refuse-to-start guards. The parent compose ships
	// `dev-quarry-control-key` as a default — if a prod deployment
	// inherits that without override (or leaves the key empty), the
	// orchestrator's event POSTs would either be silently accepted by
	// a well-known token (catastrophic) or silently rejected with no
	// log (silent data loss). Either path is a footgun; fail fast.
	if isProd {
		if apiKey == "" {
			log.Fatal().Msg(
				"REFUSING TO START: ENVIRONMENT=" + envName +
					" but QUARRY_CONTROL_API_KEY is empty. Set a strong, " +
					"unique value before deploying.",
			)
		}
		if apiKey == "dev-quarry-control-key" {
			log.Fatal().Msg(
				"REFUSING TO START: ENVIRONMENT=" + envName +
					" but QUARRY_CONTROL_API_KEY equals the well-known dev " +
					"default 'dev-quarry-control-key'. Override with a strong, " +
					"unique value.",
			)
		}
	} else if apiKey == "" {
		log.Warn().Msg("QUARRY_CONTROL_API_KEY empty — event ingestion endpoint will reject all requests")
	}
	// D2 / cluster #14 — HMAC cross-plane auth. When the secret is
	// unset, the verifier degrades to "trust the network"; once it's
	// set, we run in require=false mode for the rollout window so
	// existing callers that don't yet sign aren't broken. Flip
	// QUARRY_INTERNAL_HMAC_REQUIRED=1 to switch to enforce mode after
	// every edge instance is signing.
	internalSecret := os.Getenv("QUARRY_INTERNAL_SECRET")
	internalRequire := isProd || os.Getenv("QUARRY_INTERNAL_HMAC_REQUIRED") == "1"
	// In production we must not run in "rollout / trust-the-network"
	// mode. If the secret is missing, the HMAC verifier degrades to a
	// no-op and any unauthenticated caller on the internal network can
	// reach control's HMAC-protected endpoints. Force-fail so the
	// operator can't silently deploy with auth off.
	if isProd && internalSecret == "" {
		log.Fatal().Msg(
			"REFUSING TO START: ENVIRONMENT=" + envName +
				" but QUARRY_INTERNAL_SECRET is empty. The HMAC verifier " +
				"would degrade to allow-all on internal networks — set the " +
				"secret before deploying.",
		)
	}
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
	var dbPinger httpx.Pinger
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
		// Expose ping for /ready. pg.New returns a store.DB interface,
		// so type-assert to the concrete `*postgresDB` to grab Ping.
		if p, ok := pgDB.(httpx.Pinger); ok {
			dbPinger = p
		}
	}

	r := chi.NewRouter()
	r.Use(httpx.RequestID, httpx.Logger, httpx.Recover)

	r.Get("/health", httpx.Health)
	r.Get("/version", httpx.Version("quarry-control"))
	r.Get("/ready", httpx.ReadyWithPing(dbPinger))

	// Internal API routes — every /v1/* path expects HMAC-signed
	// requests once the secret is set. /health and /ready stay
	// outside the middleware so probes work without secrets.
	r.Group(func(r chi.Router) {
		r.Use(hmacVerifier.Middleware)

		resources.MountJobs(r, db)
		resources.MountStores(r, db)
		// Legacy snapshot CRUD was folded into MountSnapshotsV2 below — the
		// enriched list is the only /v1/snapshots surface; restore reads the
		// legacy store internally.
		resources.MountArtifacts(r, db)
		resources.MountProfiles(r, db)
		resources.MountSchedules(r, db)
		resources.MountRestore(r, db)
		resources.MountEvents(r, db, apiKey, notifySink)
		resources.MountWebhooks(r, db)
		// Edge change-webhook receiver (docs/CHANGE_TRACKING.md §"Webhook
		// emission"): signature + stale-ts + nonce replay are enforced by
		// the hmacVerifier.Middleware above — mount here, not outside the
		// group, so the handler inherits verification.
		resources.MountChangeWebhook(r, db, notifySink)
		resources.MountBlocklists(r, db)
		resources.MountWebhookDeliveries(r, db)
		resources.MountPresets(r)

		// Cycle 23/24 additions — REST resource breadth part 2 +
		// schedule lifecycle aliases. /v1/sources is a real,
		// org-scoped CRUD over `quarry_sources`; team aggregates,
		// activity, snapshots and request-queues serve their full
		// quarry_core wire shapes (cycle 24 parity work).
		//
		// Schedule trigger/backfill now take a temporal.Client
		// (F3 follow-up). Until the SDK is wired we pass nil —
		// the handler returns a typed 501 UNSUPPORTED envelope
		// rather than the previous 202-stub "accepted" lie.
		// /v1/benchmarks was removed (F3): no source of truth,
		// so the empty-page 200 was dishonest.
		resources.MountSources(r, db)
		resources.MountSnapshotsV2(r, db) // replaces MountSnapshots at GET /v1/snapshots
		resources.MountRequestQueuesV2(r, db)
		resources.MountTeam(r, db)
		resources.MountScheduleAliases(r, db, nil)
		resources.MountJobsByKind(r, db)
	})

	dCtx, dCancel := context.WithCancel(context.Background())
	go dispatcher.Run(dCtx, db, nil, dispatcher.Options{Workers: 4}, log.Logger)
	// Phase 7 retention sweep — INERT unless QUARRY_RETENTION_DAYS>0, and only
	// deletes when QUARRY_RETENTION_DRY_RUN=false; otherwise a default-safe no-op.
	go janitor.Run(dCtx, db, janitor.OptionsFromEnv(), log.Logger)

	// GDPR cross-plane org-erasure purge consumer — subscribes to
	// verevon.gdpr.erasure.requested (org-core, fanned out over the
	// control-shared-nats broker) and hard-purges this org's crawl data.
	// Uses a narrowly-scoped "quarry-control-gdpr" shared-broker identity
	// (NATS_SHARED_URL/NATS_SHARED_USER/NATS_SHARED_PASSWORD), distinct from
	// VEREVON_NATS_URL/VEREVON_NATS_TOKEN — those names are reserved elsewhere
	// in this plane for the legacy token-only verevon-nats broker, a
	// different broker from control-shared-nats. Safe by default: absent
	// NATS_SHARED_URL the consumer is simply not started, same "off by
	// default, log why" posture as the retention janitor above and the
	// notify sink.
	var gdprNC *nats.Conn
	if sharedURL := strings.TrimSpace(os.Getenv("NATS_SHARED_URL")); sharedURL != "" {
		sharedUser := strings.TrimSpace(os.Getenv("NATS_SHARED_USER"))
		sharedPassword := os.Getenv("NATS_SHARED_PASSWORD")
		nc, err := nats.Connect(sharedURL,
			nats.Name("quarry-control-gdpr"),
			nats.UserInfo(sharedUser, sharedPassword),
			nats.CustomInboxPrefix("_INBOX.QUARRY_CONTROL_GDPR"),
		)
		if err != nil {
			log.Warn().Err(err).Msg("control-shared NATS connect failed — GDPR org-erasure purge consumer disabled")
		} else if _, err := gdpr.StartSubscriber(nc, db, log.Logger); err != nil {
			log.Warn().Err(err).Msg("GDPR org-erasure subscriber failed to start")
			nc.Close()
		} else {
			gdprNC = nc
		}
	} else {
		log.Warn().Msg("NATS_SHARED_URL unset — GDPR org-erasure purge consumer disabled (org deletions will not auto-purge this service's crawl data for that org)")
	}

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

	// Shutdown order matters: drain HTTP first (so in-flight handlers
	// can finish their fanoutWebhooks → db.WebhookDeliveries().Create
	// writes), THEN cancel the dispatcher. If we cancel the dispatcher
	// first, those deliveries land in `pending` with no worker to
	// claim them until next process start.
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	_ = srv.Shutdown(ctx)
	dCancel()
	if gdprNC != nil {
		gdprNC.Close()
	}
	log.Info().Msg("shutdown complete")
}

func envOr(k, d string) string {
	if v := os.Getenv(k); v != "" {
		return v
	}
	return d
}
