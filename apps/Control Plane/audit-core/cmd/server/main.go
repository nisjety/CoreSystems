// audit-core entry point.
//
// Subscribes to NATS audit + usage subjects, persists to Postgres, and
// serves a read API at /v1/audit, /v1/usage, /v1/usage/summary. All
// reads are org-scoped (`?org_id=...` required) — the multi-tenant
// trust contract assumes the caller (verevon) attaches the verified
// org_id from the auth-core JWT.
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
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/controlplane/audit-core/internal/api"
	"github.com/triodelab/controlplane/audit-core/internal/chain"
	metricsserver "github.com/triodelab/controlplane/audit-core/internal/metrics"
	"github.com/triodelab/controlplane/audit-core/internal/store"
	"github.com/triodelab/controlplane/audit-core/internal/subscriber"
)

func main() {
	zerolog.TimeFieldFormat = zerolog.TimeFormatUnix
	log.Logger = zerolog.New(os.Stdout).With().Timestamp().Str("service", "audit-core").Logger()

	cfg, err := loadConfig()
	if err != nil {
		log.Fatal().Err(err).Msg("config load failed")
	}

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	pool, err := pgxpool.New(ctx, cfg.DatabaseURL)
	if err != nil {
		log.Fatal().Err(err).Msg("postgres connect failed")
	}
	defer pool.Close()
	if err := pool.Ping(ctx); err != nil {
		log.Fatal().Err(err).Msg("postgres ping failed")
	}

	if err := store.Migrate(ctx, pool); err != nil {
		log.Fatal().Err(err).Msg("schema migration failed")
	}
	log.Info().Msg("schema migrated (idempotent CREATE IF NOT EXISTS)")

	var storeOpts []store.Option
	if cfg.ChainEnabled {
		storeOpts = append(storeOpts, store.WithChain())
	}
	if cfg.KeyRing != nil {
		storeOpts = append(storeOpts, store.WithChainKeys(cfg.KeyRing), store.WithChainPrune(cfg.PruneMode))
	}
	st := store.New(pool, storeOpts...)
	if cfg.ChainEnabled {
		// Before the subscriber starts, so rows that landed unchained while the
		// chain was off are chained ahead of new traffic. A failure leaves them
		// for the next boot; live appends chain correctly either way.
		catchUpCtx, catchUpCancel := context.WithTimeout(ctx, 5*time.Minute)
		chained, err := st.CatchUp(catchUpCtx)
		catchUpCancel()
		if err != nil {
			log.Error().Err(err).Int64("chained", chained).Msg("audit chain catch-up failed")
		} else {
			log.Info().Int64("chained", chained).Msg("audit chain catch-up complete")
		}
	}

	natsOpts := []nats.Option{
		nats.Name("audit-core"),
		nats.CustomInboxPrefix(auditInboxPrefix("control")),
		nats.RetryOnFailedConnect(true),
		nats.MaxReconnects(-1),
		nats.ReconnectWait(2 * time.Second),
	}
	if cfg.NATSUser != "" {
		natsOpts = append(natsOpts, nats.UserInfo(cfg.NATSUser, cfg.NATSPassword))
	} else if cfg.NATSToken != "" {
		natsOpts = append(natsOpts, nats.Token(cfg.NATSToken))
	}
	nc, err := nats.Connect(cfg.NATSURL, natsOpts...)
	if err != nil {
		log.Fatal().Err(err).Str("nats_url", cfg.NATSURL).Msg("nats connect failed")
	}
	defer func() { _ = nc.Drain() }()

	sub := subscriber.New(nc, st)
	if err := sub.Start(ctx); err != nil {
		log.Fatal().Err(err).Msg("subscriber start failed")
	}

	// Cross-plane buses are explicitly named and independently credentialed.
	// Their supervisors tolerate initial unavailability and reconnect without
	// changing durable consumer or inbox identities; readiness remains degraded
	// until both durable consumers on every configured bus are observable.
	extraBuses := make([]*managedExtraNATSBus, 0, len(cfg.ExtraNATSBuses))
	for _, bus := range cfg.ExtraNATSBuses {
		managed := startManagedExtraNATSBus(ctx, bus, st)
		extraBuses = append(extraBuses, managed)
		defer managed.Close()
	}

	// Retention: enforce the AUDIT_RETENTION_DAYS window the verevon settings
	// UI advertises ("Audit retention 365 days"). The goroutine purges once at
	// startup, then daily, and exits when ctx is cancelled on shutdown.
	go runRetention(ctx, st, cfg.RetentionDays)
	if cfg.ChainEnabled && cfg.KeyRing.CanSign() {
		go runAttestor(ctx, st, cfg.AttestInterval)
	}

	r := chi.NewRouter()
	auditAPI, err := api.New(st, cfg.ServiceCredentials, func(requestContext context.Context) api.Readiness {
		checkContext, checkCancel := context.WithTimeout(requestContext, 2*time.Second)
		defer checkCancel()
		databaseConnected := pool.Ping(checkContext) == nil
		primaryConnected := nc.IsConnected()
		metricsserver.SetNATSConnected("primary", primaryConnected)
		buses := make([]api.NATSBusReadiness, 0, len(extraBuses)+1)
		buses = append(buses, subscriberReadiness("primary", primaryConnected, true, sub))
		for _, bus := range extraBuses {
			buses = append(buses, bus.Readiness())
		}
		return api.Readiness{
			DatabaseConnected: databaseConnected,
			NATSBuses:         buses,
			DeliveryMode:      "jetstream_durable",
			LagMetric:         "time() - audit_core_event_last_processed_timestamp_seconds",
		}
	})
	if err != nil {
		log.Fatal().Err(err).Msg("service credential configuration invalid")
	}
	auditAPI.Mount(r)

	addr := fmt.Sprintf("0.0.0.0:%d", cfg.HTTPPort)
	srv := &http.Server{Addr: addr, Handler: r, ReadHeaderTimeout: 5 * time.Second}
	go func() {
		log.Info().Str("addr", addr).Msg("audit-core http listening")
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Fatal().Err(err).Msg("http server error")
		}
	}()

	// Prometheus /metrics on a dedicated port (default 9091), scraped by the
	// Control-Plane Prometheus (Phase 6 B13).
	metricsSrv := metricsserver.NewServer(cfg.MetricsPort)
	go func() {
		if err := metricsSrv.Start(); err != nil {
			log.Error().Err(err).Msg("metrics server error")
		}
	}()

	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	<-quit
	log.Info().Msg("shutting down")
	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer shutdownCancel()
	_ = srv.Shutdown(shutdownCtx)
	_ = metricsSrv.Shutdown(shutdownCtx)
}

// retentionInterval is how often the retention sweep runs. The window is
// coarse (daily) by design — purging is a maintenance task, not a hot path.
const retentionInterval = 24 * time.Hour

// runRetention purges audit + usage events older than retentionDays. It runs
// one sweep immediately so a fresh boot reclaims any backlog, then ticks
// daily. It returns when ctx is cancelled (graceful shutdown).
func runRetention(ctx context.Context, st *store.Store, retentionDays int) {
	purge := func() {
		// Bound each sweep so a slow purge can't block shutdown indefinitely.
		sweepCtx, cancel := context.WithTimeout(ctx, 5*time.Minute)
		defer cancel()
		res, err := st.Purge(sweepCtx, retentionDays)
		if err != nil {
			log.Error().Err(err).Int("retention_days", retentionDays).
				Int64("audit_deleted", res.AuditDeleted).Msg("retention purge failed")
			return
		}
		log.Info().
			Int("retention_days", retentionDays).
			Int64("audit_deleted", res.AuditDeleted).
			Int64("usage_deleted", res.UsageDeleted).
			Int("checkpoints_written", res.CheckpointsWritten).
			Int("orgs_blocked", res.OrgsBlocked).
			Int64("chained_planned", res.ChainedPlanned).
			Msg("retention purge complete")
	}

	purge()

	ticker := time.NewTicker(retentionInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			log.Info().Msg("retention loop stopping")
			return
		case <-ticker.C:
			purge()
		}
	}
}

// runAttestor signs the head of every org chain each interval, so the newest
// attested sequence trails the live head by at most one interval. It returns
// when ctx is cancelled.
func runAttestor(ctx context.Context, st *store.Store, interval time.Duration) {
	attest := func() {
		attestCtx, cancel := context.WithTimeout(ctx, interval)
		defer cancel()
		written, err := st.Attest(attestCtx)
		if err != nil {
			log.Error().Err(err).Int("attested", written).Msg("audit chain head attestation failed")
		} else if written > 0 {
			log.Info().Int("attested", written).Msg("audit chain heads attested")
		}
	}

	attest()
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			log.Info().Msg("attestor stopping")
			return
		case <-ticker.C:
			attest()
		}
	}
}

type config struct {
	DatabaseURL        string
	NATSURL            string
	NATSUser           string
	NATSPassword       string
	NATSToken          string
	HTTPPort           int
	MetricsPort        int
	ServiceCredentials string
	RetentionDays      int
	ChainEnabled       bool
	// KeyRing is nil unless the chain, chained pruning or its dry run is on.
	KeyRing        *chain.KeyRing
	PruneMode      store.PruneMode
	AttestInterval time.Duration
	ExtraNATSBuses []extraNATSBus
}

// parseChainEnabled is strict: only the literal "1" turns the audit hash chain
// on, so an unset, empty or misspelled value leaves it off.
func parseChainEnabled(raw string) bool {
	return parseStrictSwitch(raw)
}

// parseStrictSwitch is the one switch parser of this service: only the literal
// "1" enables, so an unset, empty or misspelled value leaves the feature off.
func parseStrictSwitch(raw string) bool {
	return raw == "1"
}

// defaultAttestInterval is how often chain heads are signed when
// AUDIT_CHAIN_ATTEST_INTERVAL is unset or invalid.
const defaultAttestInterval = 60 * time.Second

// loadChainKeys builds the signing key ring, but only when something needs it:
// with the chain, pruning and the dry run all off the key variables are not
// read at all, so a deployment that has not opted in behaves exactly as before.
// Once something needs it, an undecodable key fails boot, and so does a missing
// private key where it cannot be skipped: chained pruning always signs, and a
// production chain must be attestable.
func loadChainKeys(chainEnabled bool, prune store.PruneMode, production bool) (*chain.KeyRing, error) {
	if !chainEnabled && prune == store.PruneOff {
		return nil, nil
	}
	ring, err := chain.NewKeyRing(chain.KeyConfig{
		KeyID:      os.Getenv("AUDIT_CHAIN_KEY_ID"),
		Private:    os.Getenv("AUDIT_CHAIN_PRIVATE_KEY_BASE64"),
		Public:     os.Getenv("AUDIT_CHAIN_PUBLIC_KEY_BASE64"),
		VerifyKeys: os.Getenv("AUDIT_CHAIN_VERIFY_KEYS"),
	})
	if err != nil {
		return nil, fmt.Errorf("audit chain key: %w", err)
	}
	if prune == store.PruneOn && !ring.CanSign() {
		return nil, fmt.Errorf("AUDIT_CHAIN_PRUNE_ENABLED=1 needs AUDIT_CHAIN_KEY_ID and AUDIT_CHAIN_PRIVATE_KEY_BASE64: every purge batch signs a checkpoint")
	}
	if chainEnabled && production && !ring.CanSign() {
		return nil, fmt.Errorf("AUDIT_CHAIN_ENABLED=1 in production needs AUDIT_CHAIN_KEY_ID and AUDIT_CHAIN_PRIVATE_KEY_BASE64: unsigned chain heads cannot be attested")
	}
	if chainEnabled && !ring.CanSign() {
		log.Warn().Msg("audit chain is on without a signing key: heads are not attested and chained rows cannot be pruned")
	}
	return ring, nil
}

// parsePruneMode maps the two switches to one mode. The dry run wins over the
// enable switch, so setting it can never delete.
func parsePruneMode(enabled, dryRun string) store.PruneMode {
	switch {
	case parseStrictSwitch(dryRun):
		return store.PruneDryRun
	case parseStrictSwitch(enabled):
		return store.PruneOn
	default:
		return store.PruneOff
	}
}

func loadConfig() (*config, error) {
	dsn := os.Getenv("DATABASE_URL")
	if dsn == "" {
		return nil, fmt.Errorf("DATABASE_URL is required")
	}
	natsURL := os.Getenv("NATS_URL")
	if natsURL == "" {
		return nil, fmt.Errorf("NATS_URL is required")
	}
	if err := validateNATSURL(natsURL); err != nil {
		return nil, fmt.Errorf("NATS_URL: %w", err)
	}
	allowTokenFallback := os.Getenv("AUDIT_ALLOW_NATS_TOKEN_FALLBACK") == "1"
	credential, err := selectNATSCredential(
		os.Getenv("NATS_USER"),
		os.Getenv("NATS_PASSWORD"),
		os.Getenv("NATS_TOKEN"),
		allowTokenFallback,
	)
	if err != nil {
		return nil, fmt.Errorf("primary NATS credential: %w", err)
	}
	port := 8187 // default listen port; override with HTTP_PORT
	if v := os.Getenv("HTTP_PORT"); v != "" {
		var p int
		if _, err := fmt.Sscanf(v, "%d", &p); err == nil && p > 0 {
			port = p
		}
	}
	metricsPort := 9091 // default Prometheus scrape port; override with METRICS_PORT
	if v := os.Getenv("METRICS_PORT"); v != "" {
		var p int
		if _, err := fmt.Sscanf(v, "%d", &p); err == nil && p > 0 {
			metricsPort = p
		}
	}
	serviceCredentials := os.Getenv("AUDIT_CORE_SERVICE_CREDENTIALS")
	if err := api.ValidateRequiredServiceCredentialRegistry(serviceCredentials); err != nil {
		return nil, fmt.Errorf("service credential registry: %w", err)
	}

	// Retention window for both append-only tables. Defaults to 365 days to
	// match the verevon settings UI. Values below 1 are clamped to 1 so a
	// misconfiguration can never purge everything on the next sweep.
	retentionDays := 365
	if v := os.Getenv("AUDIT_RETENTION_DAYS"); v != "" {
		var d int
		if _, err := fmt.Sscanf(v, "%d", &d); err == nil && d > 0 {
			retentionDays = d
		} else {
			log.Warn().Str("AUDIT_RETENTION_DAYS", v).Int("default_days", retentionDays).
				Msg("invalid AUDIT_RETENTION_DAYS; using default")
		}
	}

	chainEnabled := parseChainEnabled(os.Getenv("AUDIT_CHAIN_ENABLED"))
	pruneMode := parsePruneMode(os.Getenv("AUDIT_CHAIN_PRUNE_ENABLED"), os.Getenv("AUDIT_CHAIN_PRUNE_DRYRUN"))
	keyRing, err := loadChainKeys(chainEnabled, pruneMode, os.Getenv("ENVIRONMENT") == "production")
	if err != nil {
		return nil, err
	}
	attestInterval := defaultAttestInterval
	if v := os.Getenv("AUDIT_CHAIN_ATTEST_INTERVAL"); v != "" {
		if d, err := time.ParseDuration(v); err == nil && d >= time.Second {
			attestInterval = d
		} else {
			log.Warn().Str("AUDIT_CHAIN_ATTEST_INTERVAL", v).Dur("default", attestInterval).
				Msg("invalid AUDIT_CHAIN_ATTEST_INTERVAL; using default")
		}
	}

	if os.Getenv("EXTRA_NATS_URLS") != "" {
		return nil, fmt.Errorf("EXTRA_NATS_URLS is unsupported; configure named AUDIT_EXTRA_NATS_BUSES")
	}
	extraNATSBuses, err := parseExtraNATSBuses(os.Getenv("AUDIT_EXTRA_NATS_BUSES"), allowTokenFallback)
	if err != nil {
		return nil, err
	}

	return &config{
		DatabaseURL:        dsn,
		NATSURL:            natsURL,
		NATSUser:           credential.User,
		NATSPassword:       credential.Password,
		NATSToken:          credential.Token,
		HTTPPort:           port,
		MetricsPort:        metricsPort,
		ServiceCredentials: serviceCredentials,
		RetentionDays:      retentionDays,
		ChainEnabled:       chainEnabled,
		KeyRing:            keyRing,
		PruneMode:          pruneMode,
		AttestInterval:     attestInterval,
		ExtraNATSBuses:     extraNATSBuses,
	}, nil
}
