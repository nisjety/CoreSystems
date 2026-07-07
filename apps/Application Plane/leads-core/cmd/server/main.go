package main

import (
	"context"
	"errors"
	"log"
	stdhttp "net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/audit"
	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/brreg"
	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/database"
	apphttp "github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/http"
	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/integration"
	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/leads"
	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/providerleads"
)

func main() {
	ctx := context.Background()
	cfg, err := config.Load()
	if err != nil {
		log.Fatalf("leads-core: config: %v", err)
	}

	db, err := database.Connect(ctx, cfg.DatabaseURL)
	if err != nil {
		log.Fatalf("leads-core: database: %v", err)
	}
	defer db.Close()
	if err := database.RunMigrations(ctx, db); err != nil {
		log.Fatalf("leads-core: migrations: %v", err)
	}

	repository := leads.NewRepository(db.Pool)
	service := leads.NewService(repository, brreg.NewClient())

	// Provider lead sync (LinkedIn Lead Gen forms via integration-corev2's
	// actions gateway). PERSON DATA is confined to provider_leads — see
	// internal/providerleads and README.md ("PII posture").
	var syncer *providerleads.Syncer
	providerLeadRepo := providerleads.NewRepository(db.Pool)
	if cfg.IntegrationCoreURL != "" {
		syncer = providerleads.NewSyncer(integration.NewClient(integration.Config{
			BaseURL:        cfg.IntegrationCoreURL,
			InternalAPIKey: cfg.InternalAPIKey,
		}), providerLeadRepo)
	} else {
		log.Printf("leads-core: provider-lead sync disabled (INTEGRATION_CORE_URL is empty)")
	}

	// Optional best-effort per-export + per-sync-run audit → NATS → audit-core.
	if cfg.NATSURL != "" {
		publisher, perr := audit.Connect(cfg.NATSURL, cfg.NATSToken, cfg.ServiceName)
		if perr != nil {
			log.Printf("leads-core: audit publisher disabled: %v", perr)
		} else {
			defer publisher.Close()
			service.SetAudit(publisher)
			if syncer != nil {
				syncer.SetAudit(publisher)
			}
			log.Printf("leads-core: per-export audit enabled")
		}
	}

	workerCtx, workerCancel := context.WithCancel(ctx)
	defer workerCancel()
	if syncer != nil && cfg.ProviderLeadSyncEnabled {
		worker := providerleads.NewWorker(syncer, cfg.ProviderLeadSyncInterval)
		go worker.Start(workerCtx)
		log.Printf("leads-core: provider-lead sync worker enabled (interval %s)", cfg.ProviderLeadSyncInterval)
	}

	handler := apphttp.NewHandler(cfg, service)
	handler.SetProviderLeads(syncer, providerLeadRepo)
	server := apphttp.NewServer(cfg.HTTPPort, handler, cfg.InternalAPIKey)

	errCh := make(chan error, 1)
	go func() {
		errCh <- server.Start()
	}()

	stop := make(chan os.Signal, 1)
	signal.Notify(stop, syscall.SIGINT, syscall.SIGTERM)

	select {
	case sig := <-stop:
		log.Printf("leads-core: received %s", sig)
	case err := <-errCh:
		if err != nil && !errors.Is(err, stdhttp.ErrServerClosed) {
			log.Fatalf("leads-core: server: %v", err)
		}
	}

	shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := server.Shutdown(shutdownCtx); err != nil {
		log.Printf("leads-core: shutdown: %v", err)
	}
}
