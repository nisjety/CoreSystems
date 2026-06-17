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

	"github.com/I-Dacosta/AquatiqCMS/apps/social-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/social-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/social-core/internal/eventing"
	apphttp "github.com/I-Dacosta/AquatiqCMS/apps/social-core/internal/http"
	"github.com/I-Dacosta/AquatiqCMS/apps/social-core/internal/integration"
	appnats "github.com/I-Dacosta/AquatiqCMS/apps/social-core/internal/nats"
	"github.com/I-Dacosta/AquatiqCMS/apps/social-core/internal/social"
)

func main() {
	startupCtx, startupCancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer startupCancel()

	cfg, err := config.Load()
	if err != nil {
		log.Fatalf("social-core: config: %v", err)
	}

	db, err := database.Connect(startupCtx, cfg.DatabaseURL)
	if err != nil {
		log.Fatalf("social-core: database: %v", err)
	}
	defer db.Close()

	if err := database.RunMigrations(startupCtx, db); err != nil {
		log.Fatalf("social-core: migrations: %v", err)
	}

	repository := social.NewRepository(db.Pool)
	var eventPublisher social.EventPublisher
	natsClient, err := appnats.NewClient(appnats.Config{
		URL:   cfg.NATSURL,
		Token: cfg.NATSToken,
		Name:  cfg.ServiceName,
	})
	if err != nil {
		log.Printf("social-core: nats unavailable at %s: %v", cfg.NATSURL, err)
	} else {
		defer natsClient.Close()
		publisher := eventing.NewPublisher(natsClient.JS)
		if err := publisher.EnsureStream(); err != nil {
			log.Printf("social-core: ensure event stream: %v", err)
		}
		eventPublisher = publisher
	}
	integrationClient := integration.NewClient(integration.Config{
		BaseURL:        cfg.IntegrationCoreURL,
		InternalAPIKey: cfg.InternalAPIKey,
	})
	publisher := social.NewHTTPPublisher(integrationClient, nil, social.PublisherConfig{
		LinkedInAPIBaseURL:       cfg.LinkedInAPIBaseURL,
		LinkedInAPIVersion:       cfg.LinkedInAPIVersion,
		XAPIBaseURL:              cfg.XAPIBaseURL,
		InstagramGraphAPIBaseURL: cfg.InstagramGraphAPIBaseURL,
		FacebookGraphAPIBaseURL:  cfg.FacebookGraphAPIBaseURL,
		TikTokAPIBaseURL:         cfg.TikTokAPIBaseURL,
		SnapchatAPIBaseURL:       cfg.SnapchatAPIBaseURL,
	})
	service := social.NewService(
		repository,
		social.WithAccountSource(integrationClient),
		social.WithPublisher(publisher),
		social.WithEventPublisher(eventPublisher),
	)
	handler := apphttp.NewHandler(cfg, service)
	server := apphttp.NewServer(cfg.HTTPPort, handler, cfg.InternalAPIKey)

	runtimeCtx, runtimeCancel := context.WithCancel(context.Background())
	defer runtimeCancel()
	if cfg.PublishWorkerEnabled {
		worker := social.NewWorker(service, cfg.ServiceName, cfg.PublishWorkerPollInterval, cfg.PublishWorkerBatchSize)
		go worker.Start(runtimeCtx)
	}

	errCh := make(chan error, 1)
	go func() {
		errCh <- server.Start()
	}()

	stop := make(chan os.Signal, 1)
	signal.Notify(stop, syscall.SIGINT, syscall.SIGTERM)

	select {
	case sig := <-stop:
		log.Printf("social-core: received %s", sig)
	case err := <-errCh:
		if err != nil && !errors.Is(err, stdhttp.ErrServerClosed) {
			log.Fatalf("social-core: server: %v", err)
		}
	}

	runtimeCancel()
	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer shutdownCancel()
	if err := server.Shutdown(shutdownCtx); err != nil {
		log.Printf("social-core: shutdown: %v", err)
	}
}
