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
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/audit"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/dispatcher"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/gdpr"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/janitor"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/notify"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/posture"
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
	// to notification-core (Application Plane) over HTTP, signed as the
	// quarry-control principal. Unset URL or secret → the notify leg is inert
	// (nil sink); change tracking + webhooks still work.
	var notifySink notify.Sink
	notifyURL := strings.TrimSpace(os.Getenv("NOTIFICATION_CORE_URL"))
	notifySecret := strings.TrimSpace(os.Getenv("NOTIFICATION_QUARRY_CONTROL_SERVICE_TOKEN"))
	switch {
	case notifyURL == "":
		log.Warn().Msg("NOTIFICATION_CORE_URL unset — change notifications disabled (change tracking + webhooks unaffected)")
	case notifySecret == "":
		log.Warn().Msg("NOTIFICATION_QUARRY_CONTROL_SERVICE_TOKEN unset — change notifications disabled (change tracking + webhooks unaffected)")
	default:
		notifySink = notify.NewHTTPSink(notifyURL, notifySecret)
		log.Info().Str("url", notifyURL).Msg("notification-core sink wired for change notifications")
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
	// D2-050: the routes only the orchestrator calls (/v1/internal/jobs/*,
	// /v1/internal/schedules/*) are verified with their own secret, so the edge's
	// secret no longer reaches finalize. An unset QUARRY_CONTROL_INTERNAL_SECRET
	// falls back to the edge secret (no outage during the soak) with one warning;
	// remove the fallback after the soak.
	controlSecret, controlSecretWarning := httpx.ControlInternalSecretPosture(
		os.Getenv("QUARRY_CONTROL_INTERNAL_SECRET"), internalSecret)
	orchestratorVerifier := hmacVerifier
	if controlSecret != internalSecret {
		orchestratorVerifier = httpx.NewHMACVerifier(controlSecret, internalRequire)
	}
	switch controlSecretWarning {
	case "unset":
		log.Warn().Msg("QUARRY_CONTROL_INTERNAL_SECRET unset: the orchestrator-only internal routes fall back to QUARRY_INTERNAL_SECRET (shared with the edge). Set it; this fallback is removed after the soak (D2-050)")
	case "same":
		log.Warn().Msg("QUARRY_CONTROL_INTERNAL_SECRET equals QUARRY_INTERNAL_SECRET: the orchestrator-only internal routes are not isolated from the edge. Use a distinct value (D2-050)")
	default:
		log.Info().Msg("orchestrator-only internal routes verify with QUARRY_CONTROL_INTERNAL_SECRET (split from the edge secret)")
	}
	switch {
	case hmacVerifier.HasSecret() && internalRequire:
		log.Info().Msg("HMAC cross-plane auth: ENFORCED (every internal request must sign)")
	case hmacVerifier.HasSecret():
		log.Info().Msg("HMAC cross-plane auth: rollout mode (signatures verified when present)")
	default:
		log.Warn().Msg("HMAC cross-plane auth: DISABLED (no secret) — control plane MUST be on private network")
	}

	// Job admission: per-organization caps, a bounded backlog and the claim
	// endpoint. Off unless QUARRY_JOB_ADMISSION=on; a value that cannot be read
	// stops control here rather than silently running without the bound.
	admission, err := resources.AdmissionFromEnv(os.Getenv)
	if err != nil {
		log.Fatal().Err(err).Msg("job admission configuration is invalid")
	}
	log.Info().
		Bool("enabled", admission.Enabled).
		Int("cap_default", admission.CapDefault).
		Int("backlog_default", admission.BacklogDefault).
		Dur("backlog_ttl", admission.BacklogTTL).
		Msg("job admission")

	// WS14.S7b: a job that ignores robots.txt is written to audit-core
	// (crawl.robots_ignored, producer quarry-control, plane ingestion) before it
	// is acknowledged. AUDIT_CORE_URL and AUDIT_CORE_SERVICE_TOKEN both unset is
	// the default: the client is off and every such job answers 503. Exactly one
	// set, or an invalid URL, is logged and treated as off (never fatal: nothing
	// else depends on it). Jobs that respect robots.txt never touch it.
	jobsOpts := []resources.JobsOption{resources.WithAdmission(admission)}
	robotsAudit, err := audit.FromEnv(os.Getenv, nil)
	switch {
	case err != nil:
		log.Warn().Err(err).Msg("audit-core client misconfigured: jobs that ignore robots.txt are refused")
	case robotsAudit == nil:
		log.Info().Msg("audit-core client off (AUDIT_CORE_URL unset): jobs that ignore robots.txt are refused")
	default:
		jobsOpts = append(jobsOpts, resources.WithRobotsAudit(robotsAudit))
		log.Info().Msg("audit-core client on: robots.txt overrides are audited before the job is acknowledged")
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
	// WS10.S8 (D2-021): one request span, so quarryotel.MarkZDR in the handlers has a span to flag.
	r.Use(httpx.ZdrSpan)

	r.Get("/health", httpx.Health)
	r.Get("/version", httpx.Version("quarry-control"))
	r.Get("/ready", httpx.ReadyWithPing(dbPinger))

	// Internal API routes — every /v1/* path expects HMAC-signed
	// requests once the secret is set. /health and /ready stay
	// outside the middleware so probes work without secrets.
	r.Group(func(r chi.Router) {
		r.Use(httpx.SplitHMAC(orchestratorVerifier, hmacVerifier))

		resources.MountJobs(r, db, jobsOpts...)
		resources.MountStores(r, db)
		// Legacy snapshot CRUD was folded into MountSnapshotsV2 below — the
		// enriched list is the only /v1/snapshots surface; restore reads the
		// legacy store internally.
		resources.MountArtifacts(r, db)
		resources.MountProfiles(r, db)
		resources.MountSchedules(r, db)
		resources.MountRestore(r, db)
		resources.MountEvents(r, db, apiKey, notifySink)
		// INNH-5: the unscoped webhook CRUD and delivery routes are retired (410).
		resources.MountWebhooksGone(r)
		// INNH-5: the org-scoped subscription API. OFF BY DEFAULT: with the flag unset
		// the path is not mounted (404) and no delivery retention sweep runs.
		if resources.WebhookSubscriptionsEnabled(os.Getenv) {
			resources.MountWebhookSubscriptions(r, db, resources.WebhookSubscriptionOptions{})
			log.Warn().Msg("webhook subscription API is ON (QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS)")
		} else {
			log.Info().Msg("webhook subscription API is off (default): /v1/webhook-subscriptions is not mounted")
		}
		// Edge change-webhook receiver (docs/CHANGE_TRACKING.md §"Webhook
		// emission"): signature + stale-ts + nonce replay are enforced by
		// the hmacVerifier.Middleware above — mount here, not outside the
		// group, so the handler inherits verification.
		resources.MountChangeWebhook(r, db, notifySink)
		// WS4.J: the ZDR run tombstone receiver. A new egress path under ZDR, so
		// it is mounted only when this process has an HMAC secret (a trust-the-
		// network control plane has no receiver at all) AND
		// QUARRY_ZDR_TOMBSTONE_RECEIVER is on. OFF BY DEFAULT on both sides: the
		// edge emits nothing unless QUARRY_EDGE__ZDR_TOMBSTONE is on, and this
		// side neither receives nor writes an erasure marker (a retention of an
		// erased org's id) until its own flag is on. Counsel review gates turning
		// either on.
		if zdrTombstoneReceiverPosture(os.Getenv, hmacVerifier.HasSecret(), db) {
			resources.MountZdrTombstones(r, db)
			log.Warn().Msg("ZDR tombstone receiver is ON (QUARRY_ZDR_TOMBSTONE_RECEIVER): erasures now leave a marker row")
		} else {
			log.Info().Msg("ZDR tombstone receiver is off (default): no route, no erasure marker")
		}
		// WS7.F: /v1/blocklists answers 410 Gone. The CRUD mount is
		// resources.MountBlocklists(r, db); one-line rollback.
		resources.MountBlocklistsGone(r)

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
	// Job admission (off unless QUARRY_JOB_ADMISSION=on): fail jobs that waited
	// past the TTL so their streams close.
	go resources.RunBacklogJanitor(dCtx, db, admission, time.Minute, log.Logger)
	go dispatcher.Run(dCtx, db, nil, dispatcher.Options{Workers: 4}, log.Logger)
	if resources.WebhookSubscriptionsEnabled(os.Getenv) {
		days, invalid := janitor.WebhookDeliveryRetentionDaysFromEnv(os.Getenv)
		if invalid {
			log.Warn().Int("retention_days", days).Msg("QUARRY_CONTROL__WEBHOOK_DELIVERY_RETENTION_DAYS is not an integer from 1 to 3650 - using the default")
		}
		go janitor.RunWebhookDeliveries(dCtx, db, days, time.Hour, log.Logger)
	}
	// Phase 7 retention sweep — INERT unless QUARRY_RETENTION_DAYS>0, and only
	// deletes when QUARRY_RETENTION_DRY_RUN=false; otherwise a default-safe no-op.
	go janitor.Run(dCtx, db, janitor.OptionsFromEnv(), log.Logger)
	// WS4.J: the ZDR tombstone sweeper. Unlike the artifact janitor above it runs by
	// default (30 days): it deletes metadata only, nothing references a tombstone,
	// and with the edge flag off there are no rows. A bad retention value logs and
	// falls back to 30; it never disables the sweep.
	tombOpts, tombInvalid := janitor.TombstoneOptionsFromEnv(os.Getenv)
	if tombInvalid {
		log.Warn().Int("retention_days", tombOpts.RetentionDays).Msg("QUARRY_ZDR_TOMBSTONE_RETENTION_DAYS is not a non-negative integer — using the default")
	}
	go janitor.RunTombstones(dCtx, db, tombOpts, log.Logger)

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
		} else if _, err := gdpr.StartSubscriber(nc, db, gdpr.EdgePurgerFromEnv(os.Getenv, log.Logger), log.Logger); err != nil {
			log.Warn().Err(err).Msg("GDPR org-erasure subscriber failed to start")
			nc.Close()
		} else {
			gdprNC = nc
		}
	} else {
		log.Warn().Msg("NATS_SHARED_URL unset — GDPR org-erasure purge consumer disabled (org deletions will not auto-purge this service's crawl data for that org)")
	}

	// X-9: the ZDR-enabled consumer on controlplane.org.interactive_retention.enabled
	// (org-core, CONTROLPLANE stream). OFF BY DEFAULT: with
	// QUARRY_CONTROL__ZDR_EVENT_CONSUMER unset or off it is not started. observe
	// logs would_pause per schedule; enforce pauses the org's schedules and
	// change monitors and writes a skipped_zdr reason event. It shares the
	// control-shared connection above and binds a consumer audit-core must have
	// provisioned (quarry-control-interactive-retention).
	zdrMode, zdrModeInvalid := posture.ModeFromEnv(os.Getenv)
	if zdrModeInvalid {
		log.Warn().Str("flag", posture.FlagEnv).Msg("ZDR-enabled consumer flag is not off, observe or enforce - consumer stays off")
	}
	switch {
	case zdrMode == posture.ModeOff:
		log.Info().Msg("ZDR-enabled consumer is off (default): schedules keep their create-time ZDR stamp (X-9)")
	case gdprNC == nil:
		log.Warn().Str("mode", string(zdrMode)).Msg("ZDR-enabled consumer requested but the control-shared NATS connection is not up - consumer disabled")
	default:
		if _, err := posture.StartSubscriber(gdprNC, posture.NewConsumer(db, zdrMode, log.Logger), log.Logger); err != nil {
			log.Warn().Err(err).Str("mode", string(zdrMode)).Msg("ZDR-enabled consumer failed to start")
		} else {
			log.Warn().Str("mode", string(zdrMode)).Msg("ZDR-enabled consumer is ON (QUARRY_CONTROL__ZDR_EVENT_CONSUMER)")
		}
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

// zdrTombstoneReceiverPosture decides whether the tombstone receiver is mounted and,
// when it is, switches on PurgeOrg's erasure marker. Both are OFF unless
// QUARRY_ZDR_TOMBSTONE_RECEIVER is truthy AND the process has an HMAC secret; with
// either missing no marker is ever written, so a fresh deploy retains nothing of an
// erased org's id. Counsel review gates setting the flag.
func zdrTombstoneReceiverPosture(getenv func(string) string, hasSecret bool, db store.DB) bool {
	if !janitor.TombstoneReceiverEnabled(getenv) || !hasSecret {
		return false
	}
	db.ZdrTombstones().SetErasureMarkers(true)
	return true
}

func envOr(k, d string) string {
	if v := os.Getenv(k); v != "" {
		return v
	}
	return d
}
