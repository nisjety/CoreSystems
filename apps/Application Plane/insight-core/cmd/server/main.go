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
	apphttp "github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/http"
	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/insights"
)

func main() {
	cfg, err := config.Load()
	if err != nil {
		log.Fatalf("insight-core: config: %v", err)
	}

	repository := insights.NewMemoryRepository(insights.DefaultConnectorSlots(insights.ConnectorSlotOptions{
		GoogleAnalyticsAPIBaseURL:     cfg.GoogleAnalyticsAPIBaseURL,
		GoogleSearchConsoleAPIBaseURL: cfg.GoogleSearchConsoleAPIBaseURL,
		TokenLeaseAudience:            cfg.ConnectorTokenLeaseAudience,
	}))
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
