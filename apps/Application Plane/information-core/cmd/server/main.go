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

	"coresystem/apps/application-plane/information-core/internal/address"
	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/config"
	"coresystem/apps/application-plane/information-core/internal/datex"
	"coresystem/apps/application-plane/information-core/internal/exchange"
	"coresystem/apps/application-plane/information-core/internal/frost"
	"coresystem/apps/application-plane/information-core/internal/geospatial"
	apphttp "coresystem/apps/application-plane/information-core/internal/http"
	"coresystem/apps/application-plane/information-core/internal/journey"
	"coresystem/apps/application-plane/information-core/internal/legal"
	"coresystem/apps/application-plane/information-core/internal/news"
	"coresystem/apps/application-plane/information-core/internal/parliament"
	"coresystem/apps/application-plane/information-core/internal/statistics"
	"coresystem/apps/application-plane/information-core/internal/traffic"
	"coresystem/apps/application-plane/information-core/internal/weather"
)

func main() {
	cfg := config.Load()

	cacheStore := cache.New()
	httpClient := &http.Client{Timeout: 20 * time.Second}

	handler := apphttp.NewHandler(
		cfg,
		address.NewService(httpClient, cacheStore),
		news.NewService(httpClient, cacheStore),
		traffic.NewService(httpClient, cacheStore),
		weather.NewService(httpClient, cacheStore),
	)
	handler.SetNorwaySources(apphttp.NorwaySources{
		Statistics: statistics.NewService(httpClient, cacheStore),
		Journey:    journey.NewService(httpClient, cacheStore, cfg.EnturClientName),
		Parliament: parliament.NewService(httpClient, cacheStore),
		Legal:      legal.NewService(httpClient, cacheStore, cfg.LovdataAPIKey),
		Exchange:   exchange.NewService(httpClient, cacheStore),
		Geospatial: geospatial.NewService(httpClient, cacheStore),
		Datex:      datex.NewServiceWithCache(httpClient, cacheStore, cfg.DatexURL, cfg.DatexUsername, cfg.DatexPassword),
		Frost:      frost.NewServiceWithCache(httpClient, cacheStore, cfg.FrostURL, cfg.FrostClientID),
	})
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
