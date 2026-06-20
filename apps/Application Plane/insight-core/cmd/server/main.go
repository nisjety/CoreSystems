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

	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/database"
	apphttp "github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/http"
	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/insights"
)

func main() {
	cfg, err := config.Load()
	if err != nil {
		log.Fatalf("insight-core: config: %v", err)
	}

	connectors := insights.DefaultConnectorSlots(insights.ConnectorSlotOptions{
		GoogleAnalyticsAPIBaseURL:     cfg.GoogleAnalyticsAPIBaseURL,
		GoogleSearchConsoleAPIBaseURL: cfg.GoogleSearchConsoleAPIBaseURL,
		TokenLeaseAudience:            cfg.ConnectorTokenLeaseAudience,
	})

	// W3: durable metric store when DATABASE_URL is set; otherwise the Phase-1
	// in-memory repo (registry-only, metrics surfaced as an explicit empty-state).
	var repository insights.Repository = insights.NewMemoryRepository(connectors)
	if cfg.DatabaseURL != "" {
		ctx := context.Background()
		db, err := database.Connect(ctx, cfg.DatabaseURL)
		if err != nil {
			log.Fatalf("insight-core: database: %v", err)
		}
		defer db.Close()
		if err := database.RunMigrations(ctx, db); err != nil {
			log.Fatalf("insight-core: migrations: %v", err)
		}
		repository = insights.NewPGRepository(db.Pool, connectors)
		log.Printf("insight-core: durable metric store (Postgres) enabled")
	} else {
		log.Printf("insight-core: in-memory metric store (set DATABASE_URL for durable metrics)")
	}
	service := insights.NewService(repository)
	handler := apphttp.NewHandler(cfg, service)
	server := apphttp.NewServer(cfg.HTTPPort, handler, cfg.InternalAPIKey)

	errCh := make(chan error, 1)
	go func() {
		errCh <- server.Start()
	}()

	stop := make(chan os.Signal, 1)
	signal.Notify(stop, syscall.SIGINT, syscall.SIGTERM)

	select {
	case sig := <-stop:
		log.Printf("insight-core: received %s", sig)
	case err := <-errCh:
		if err != nil && !errors.Is(err, stdhttp.ErrServerClosed) {
			log.Fatalf("insight-core: server: %v", err)
		}
	}

	shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := server.Shutdown(shutdownCtx); err != nil {
		log.Printf("insight-core: shutdown: %v", err)
	}
}
