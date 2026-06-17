package main

import (
	"context"
	"errors"
	"log"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/config"
	apphttp "coresystem/apps/application-plane/information-core/internal/http"
	"coresystem/apps/application-plane/information-core/internal/news"
	"coresystem/apps/application-plane/information-core/internal/shipping"
	"coresystem/apps/application-plane/information-core/internal/traffic"
	"coresystem/apps/application-plane/information-core/internal/weather"
)

func main() {
	cfg := config.Load()

	cacheStore := cache.New()
	httpClient := &http.Client{Timeout: 20 * time.Second}

	handler := apphttp.NewHandler(
		cfg,
		news.NewService(httpClient, cacheStore),
		shipping.NewService(httpClient, cacheStore, cfg.BringAPIUID, cfg.BringAPIKey),
		traffic.NewService(httpClient, cacheStore),
		weather.NewService(httpClient, cacheStore),
	)
	server := apphttp.NewServer(cfg, handler)

	errCh := make(chan error, 1)
	go func() {
		errCh <- server.Start()
	}()

	stop := make(chan os.Signal, 1)
	signal.Notify(stop, syscall.SIGINT, syscall.SIGTERM)

	select {
	case sig := <-stop:
		log.Printf("information-core: received %s", sig)
	case err := <-errCh:
		if err != nil && !errors.Is(err, http.ErrServerClosed) {
			log.Fatalf("information-core: server: %v", err)
		}
	}

	shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if err := server.Shutdown(shutdownCtx); err != nil {
		log.Printf("information-core: shutdown: %v", err)
	}
}
