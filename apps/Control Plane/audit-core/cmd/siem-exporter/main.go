// siem-exporter entry point (build plan WS06-S8).
//
// A separate binary from the audit-core server: it dials addresses that
// customers chose, so it holds no ingest credential, no signing key and no NATS
// access. It reads destinations from org-core (service credential, scope
// org:siem:read:any) and each org's audit chain from the audit database, and
// delivers in chain order with a cursor that advances only after a 2xx.
//
// AUDIT_SIEM_EXPORT_ENABLED is off unless it is exactly "1". Off, the process
// serves its metrics and does nothing else, so a compose stack that includes
// the siem profile without opting in exports nothing.
package main

import (
	"context"
	"errors"
	"fmt"
	"os"
	"os/signal"
	"strconv"
	"syscall"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"

	metricsserver "github.com/triodelab/controlplane/audit-core/internal/metrics"
	"github.com/triodelab/controlplane/audit-core/internal/siem"
)

const (
	// orgCoreServiceID is the principal org-core's credential registry knows
	// this process as; the compose block grants it only org:siem:read:any.
	orgCoreServiceID = "audit-core"
	defaultMetrics   = 9091
	schemaWait       = 5 * time.Second
)

type config struct {
	Enabled      bool
	AllowPrivate bool
	DatabaseURL  string
	OrgCoreURL   string
	OrgCoreToken string
	MetricsPort  int
}

// loadConfig reads the environment through getenv so a test can supply it.
// Every switch is strict: only the literal "1" enables. Setting the dev-only
// private-destination switch in production is a boot failure, not a warning,
// and it is refused even while the exporter is idle, so a misconfigured
// production stack is caught before anyone turns the exporter on.
func loadConfig(getenv func(string) string) (config, error) {
	cfg := config{
		Enabled:      getenv("AUDIT_SIEM_EXPORT_ENABLED") == "1",
		AllowPrivate: getenv("AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS") == "1",
		MetricsPort:  defaultMetrics,
	}
	if cfg.AllowPrivate && getenv("ENVIRONMENT") == "production" {
		return config{}, errors.New("AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS=1 is refused when ENVIRONMENT=production")
	}
	if raw := getenv("METRICS_PORT"); raw != "" {
		if port, err := strconv.Atoi(raw); err == nil && port > 0 {
			cfg.MetricsPort = port
		}
	}
	if !cfg.Enabled {
		return cfg, nil
	}
	cfg.DatabaseURL = getenv("DATABASE_URL")
	cfg.OrgCoreURL = getenv("ORG_CORE_URL")
	cfg.OrgCoreToken = getenv("ORG_CORE_SERVICE_TOKEN")
	for name, value := range map[string]string{
		"DATABASE_URL": cfg.DatabaseURL, "ORG_CORE_URL": cfg.OrgCoreURL, "ORG_CORE_SERVICE_TOKEN": cfg.OrgCoreToken,
	} {
		if value == "" {
			return config{}, fmt.Errorf("%s is required when AUDIT_SIEM_EXPORT_ENABLED=1", name)
		}
	}
	return cfg, nil
}

func main() {
	zerolog.TimeFieldFormat = zerolog.TimeFormatUnix
	log.Logger = zerolog.New(os.Stdout).With().Timestamp().Str("service", "siem-exporter").Logger()

	cfg, err := loadConfig(os.Getenv)
	if err != nil {
		log.Fatal().Err(err).Msg("config load failed")
	}
	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()

	metrics := metricsserver.NewServer(cfg.MetricsPort)
	go func() {
		if err := metrics.Start(); err != nil {
			log.Error().Err(err).Msg("metrics server stopped")
		}
	}()
	defer func() {
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = metrics.Shutdown(shutdownCtx)
	}()

	if !cfg.Enabled {
		log.Info().Msg("siem export is off (AUDIT_SIEM_EXPORT_ENABLED is not 1): idle")
		<-ctx.Done()
		return
	}
	if cfg.AllowPrivate {
		log.Warn().Msg("private and loopback destinations are allowed (development only)")
	}
	if err := run(ctx, cfg); err != nil && !errors.Is(err, context.Canceled) {
		log.Fatal().Err(err).Msg("siem exporter failed")
	}
}

func run(ctx context.Context, cfg config) error {
	pool, err := pgxpool.New(ctx, cfg.DatabaseURL)
	if err != nil {
		return fmt.Errorf("postgres connect failed: %w", err)
	}
	defer pool.Close()

	cursors := siem.NewPGCursors(pool)
	if err := cursors.WaitReady(ctx, schemaWait, func() {
		log.Info().Msg("waiting for audit-core to migrate audit_export_cursors")
	}); err != nil {
		return err
	}
	orgCore, err := siem.NewOrgCoreClient(cfg.OrgCoreURL, orgCoreServiceID, cfg.OrgCoreToken, nil)
	if err != nil {
		return err
	}
	runner := &siem.Runner{
		Destinations: orgCore,
		Source:       siem.NewPGSource(pool),
		Cursors:      cursors,
		Client:       siem.NewHTTPClient(siem.Egress{AllowPrivate: cfg.AllowPrivate}),
	}
	log.Info().Dur("poll_interval", siem.PollInterval).Msg("siem exporter running")
	runner.Run(ctx)
	return nil
}
