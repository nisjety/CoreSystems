package main

import (
	"context"
	"errors"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/handoff"
	"github.com/triodelab/integration-corev2/internal/workers"
)

func main() {
	zerolog.TimeFieldFormat = time.RFC3339Nano
	logger := log.Output(zerolog.ConsoleWriter{Out: os.Stdout, TimeFormat: time.RFC3339})

	cfg, err := config.Load()
	if err != nil {
		logger.Fatal().Err(err).Msg("load config")
	}
	if err := cfg.ValidateFinspoWorkerRuntime(); err != nil {
		logger.Fatal().Err(err).Msg("validate finspo worker config")
	}

	httpClient := &http.Client{Timeout: 20 * time.Second}
	// DataPlane is optional when its URL is absent. When enabled it mints a
	// short-lived, org-scoped bearer from Auth Core with this service's durable
	// credential; the durable credential is never forwarded to Data Plane.
	worker := workers.FinspoWorker{
		Integration:  handoff.NewIntegrationClientFromConfig(cfg, httpClient),
		Finspo:       handoff.NewFinspoClientFromConfig(cfg, httpClient),
		DataPlane:    handoff.NewDataPlaneDocumentsClientFromConfig(cfg, httpClient),
		PollInterval: 5 * time.Second,
		Logger:       &logger,
	}

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer cancel()

	logger.Info().Msg("starting finspo integration worker")
	if err := worker.Run(ctx); err != nil && !errors.Is(err, context.Canceled) {
		logger.Fatal().Err(err).Msg("finspo integration worker stopped with error")
	}
	logger.Info().Msg("finspo integration worker stopped")
}
