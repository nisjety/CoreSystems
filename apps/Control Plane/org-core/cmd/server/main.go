package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"net/http"
	_ "net/http/pprof"
	"os"
	"os/signal"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	grpcserver "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/grpc"
	httpserver "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/http"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/internalkey"
	metricsserver "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/metrics"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/nats"
	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/rbac"
	rediscache "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/redis"
)

func main() {
	// G40 (velion-gap.md §8.29): refuse to start in production with a
	// placeholder / missing / drifted internal API key. Mirrors velion's
	// boot-time gate (§8.27).
	if r := internalkey.AssertFromEnv("INTERNAL_API_KEY", "INTERNAL_SERVICE_SECRET"); !r.OK {
		msg := "[org-core startup] internal API key validation failed (" + string(r.Problem.Kind) + " on " + r.Problem.EnvVar + "): " + r.Problem.Detail
		if internalkey.IsProduction() {
			log.Fatalf("FATAL %s", msg)
		}
		log.Printf("WARN  %s — continuing because not production", msg)
	} else {
		log.Printf("[org-core startup] internal API key OK (%s)", r.Resolved)
	}

	// pprof debug server — enable with PPROF_ENABLED=true; default addr :6061
	if os.Getenv("PPROF_ENABLED") == "true" {
		pprofAddr := os.Getenv("PPROF_ADDR")
		if pprofAddr == "" {
			pprofAddr = ":6061"
		}
		runtime.SetMutexProfileFraction(10)
		runtime.SetBlockProfileRate(100_000_000)
		go func() {
			log.Printf("pprof debug server listening on %s", pprofAddr)
			if err := http.ListenAndServe(pprofAddr, nil); err != nil {
				log.Printf("pprof server error: %v", err)
			}
		}()
	}
	cfg, err := config.Load()
	if err != nil {
		log.Fatalf("load config: %v", err)
	}

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	db, err := database.Connect(ctx, cfg.DatabaseURL)
	if err != nil {
		log.Fatalf("connect database: %v", err)
	}
	defer db.Close()

	if err := database.RunMigrations(ctx, db, filepath.Join("migrations")); err != nil {
		log.Fatalf("run migrations: %v", err)
	}

	repo := orgcore.NewRepository(db)
	// U6-3 (ui-ux-velion-gap.md §10): role/permission editor backend.
	rbacRepo := rbac.NewRepository(db.Pool)

	// Initialize Redis cache (optional — gracefully degraded when disabled)
	var redisClient *rediscache.Client
	if cfg.Redis.Enabled {
		log.Println("🔌 Connecting to Redis (org-core)...")
		redisClient, err = rediscache.NewClient(rediscache.Config{
			Host:     cfg.Redis.Host,
			Port:     cfg.Redis.Port,
			Password: cfg.Redis.Password,
			DB:       cfg.Redis.DB,
		})
		if err != nil {
			log.Printf("⚠️  Warning: Redis unavailable — running without cache: %v", err)
			redisClient = nil
		} else {
			defer redisClient.Close()
			log.Println("✅ Redis cache connected (org-core)")
		}
	}

	var publisher orgcore.Publisher
	natsClient, err := nats.NewClient(nats.Config{
		URL:   cfg.NATSURL,
		Token: cfg.NATSToken,
		Name:  cfg.ServiceName,
	})
	if err != nil {
		log.Printf("warning: nats unavailable, running without event bridge: %v", err)
	}

	if natsClient != nil {
		defer natsClient.Close()
		if err := natsClient.EnsureStream(ctx, "CONTROL_PLANE_EVENTS", []string{"user.>", "organization.>", "session.>", "billing.>", "usage.>"}); err != nil {
			log.Printf("warning: ensure nats stream failed: %v", err)
		}
		publisher = nats.NewPublisher(natsClient)
	}

	orgService := orgcore.NewService(repo, publisher, redisClient)

	// Wire the local control-plane bus (controlplane-nats) as the audit
	// publisher. velion.audit.v1.control.* events go here — NOT the shared
	// velion-nats bus — because audit-core's primary QueueSubscribe listens on
	// controlplane-nats. Core publish (no JetStream) matches audit-core's
	// core subscription.
	if natsClient != nil {
		orgService.SetAuditPublisher(natsClient)
	}

	// Wire shared cross-plane publisher (velion-nats)
	if sp, spErr := nats.NewSharedPublisher(cfg.NATSSharedURL, cfg.NATSSharedToken, cfg.ServiceName); spErr != nil {
		log.Printf("warning: shared NATS unavailable: %v", spErr)
	} else if sp != nil {
		defer sp.Close()
		orgService.SetSharedPublisher(sp)
		log.Println("✅ org-core connected to shared NATS (velion-nats)")
	}

	var subscriber *nats.BridgeSubscriber
	if natsClient != nil {
		subscriber = nats.NewBridgeSubscriber(natsClient, publisher.(*nats.Publisher), orgService)
		if err := subscriber.Start(ctx); err != nil {
			log.Printf("warning: nats bridge subscriber failed to start: %v", err)
		}
	}

	// GDPR retention sweep: hard-delete organizations soft-deleted more than
	// ORG_PURGE_DAYS days ago (default 30) by calling the
	// purge_old_deleted_organizations stored procedure. Runs once at startup,
	// then daily, and exits when ctx is cancelled. Mirrors audit-core's
	// retention cron shape.
	go runOrgPurge(ctx, orgService, orgPurgeDays())

	server := httpserver.NewServer(cfg.HTTPPort, orgService, rbacRepo, cfg.AuthServiceURL, cfg.UserServiceURL)
	grpcServer := grpcserver.NewServer(cfg.GRPCPort)
	metricsServer := metricsserver.NewServer(cfg.MetricsPort)

	errCh := make(chan error, 3)
	go func() {
		if err := server.Start(); err != nil && !errors.Is(err, http.ErrServerClosed) {
			errCh <- fmt.Errorf("http server: %w", err)
		}
	}()
	go func() {
		if err := grpcServer.Start(); err != nil {
			errCh <- fmt.Errorf("gRPC server: %w", err)
		}
	}()
	go func() {
		if err := metricsServer.Start(); err != nil && !errors.Is(err, http.ErrServerClosed) {
			errCh <- fmt.Errorf("metrics server: %w", err)
		}
	}()

	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, os.Interrupt, syscall.SIGTERM)

	select {
	case sig := <-sigCh:
		log.Printf("shutdown signal received: %s", sig.String())
	case srvErr := <-errCh:
		if srvErr != nil {
			log.Printf("server stopped: %v", srvErr)
		}
	}

	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer shutdownCancel()
	grpcServer.Stop()
	if err := metricsServer.Shutdown(shutdownCtx); err != nil {
		log.Printf("metrics shutdown error: %v", err)
	}
	if err := server.Shutdown(shutdownCtx); err != nil {
		log.Printf("http shutdown error: %v", err)
	}
}

// orgPurgeInterval is how often the GDPR retention sweep runs. Daily by design
// — purging is a maintenance task, not a hot path.
const orgPurgeInterval = 24 * time.Hour

// orgPurgeDays reads ORG_PURGE_DAYS (default 30). Values below 1 are clamped to
// 1 so a misconfiguration can never purge everything on the next sweep.
func orgPurgeDays() int {
	days := 30
	if v := strings.TrimSpace(os.Getenv("ORG_PURGE_DAYS")); v != "" {
		var d int
		if _, err := fmt.Sscanf(v, "%d", &d); err == nil && d > 0 {
			days = d
		} else {
			log.Printf("warning: invalid ORG_PURGE_DAYS=%q; using default %d", v, days)
		}
	}
	if days < 1 {
		days = 1
	}
	return days
}

// runOrgPurge invokes purge_old_deleted_organizations(days) once immediately,
// then daily, logging the purged count. Returns when ctx is cancelled.
func runOrgPurge(ctx context.Context, svc *orgcore.Service, days int) {
	purge := func() {
		// Bound each sweep so a slow purge can't block shutdown indefinitely.
		sweepCtx, cancel := context.WithTimeout(ctx, 5*time.Minute)
		defer cancel()

		receipt, err := svc.PurgeDeletedOrganizations(sweepCtx, days)
		if err != nil {
			log.Printf("org GDPR purge failed (days=%d): %v", days, err)
			return
		}
		var parsed struct {
			PurgedCount int `json:"purged_count"`
		}
		_ = json.Unmarshal(receipt, &parsed)
		log.Printf("org GDPR purge complete: purged_count=%d days=%d", parsed.PurgedCount, days)
	}

	purge() // immediate sweep at startup
	ticker := time.NewTicker(orgPurgeInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			log.Println("org GDPR purge loop stopping")
			return
		case <-ticker.C:
			purge()
		}
	}
}
