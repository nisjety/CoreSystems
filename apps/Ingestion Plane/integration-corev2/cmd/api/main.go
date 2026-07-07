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

	"github.com/triodelab/integration-corev2/internal/actions"
	"github.com/triodelab/integration-corev2/internal/api"
	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/controlplane"
	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/db"
	"github.com/triodelab/integration-corev2/internal/discovery"
	"github.com/triodelab/integration-corev2/internal/events"
	"github.com/triodelab/integration-corev2/internal/hotpath"
	"github.com/triodelab/integration-corev2/internal/oauth"
	"github.com/triodelab/integration-corev2/internal/store"
)

func main() {
	zerolog.TimeFieldFormat = time.RFC3339Nano
	logger := log.Output(zerolog.ConsoleWriter{Out: os.Stdout, TimeFormat: time.RFC3339})

	cfg, err := config.Load()
	if err != nil {
		logger.Fatal().Err(err).Msg("load config")
	}
	if err := cfg.ValidateRuntime(); err != nil {
		logger.Fatal().Err(err).Msg("validate config")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()

	repo, cleanup, err := buildRepository(ctx, cfg)
	if err != nil {
		logger.Fatal().Err(err).Msg("initialize repository")
	}
	defer cleanup()

	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		logger.Fatal().Err(err).Msg("initialize token vault")
	}

	service := oauth.NewService(cfg, repo, vault, oauth.NewMicrosoftClient(oauth.MicrosoftClientConfig{
		ClientID:         cfg.MicrosoftClientID,
		ClientSecret:     cfg.MicrosoftClientSecret,
		ClientAuthMode:   cfg.MicrosoftClientAuthMode,
		TokenOrigin:      cfg.MicrosoftTokenOrigin,
		AuthorizationURL: cfg.MicrosoftAuthorizationURL,
		TokenURL:         cfg.MicrosoftTokenURL,
		GraphBaseURL:     cfg.MicrosoftGraphBaseURL,
		HTTPClient:       &http.Client{Timeout: 15 * time.Second},
	}))
	var publisher events.Publisher = events.NoopPublisher{}
	eventCleanup := func() {}
	if cfg.NATSEnabled {
		natsPublisher, err := events.NewNATSPublisher(cfg)
		if err != nil {
			logger.Fatal().Err(err).Msg("initialize event publisher")
		}
		publisher = natsPublisher
		service.SetEventPublisher(natsPublisher)
		eventCleanup = func() {
			if err := natsPublisher.Close(); err != nil {
				logger.Warn().Err(err).Msg("drain event publisher")
			}
		}
	}
	defer eventCleanup()
	controlPlaneClient := &http.Client{Timeout: 5 * time.Second}
	hotPathClient := &http.Client{Timeout: 2 * time.Second}
	app := api.NewServer(api.ServerConfig{
		Config:    cfg,
		Repo:      repo,
		OAuth:     service,
		Auth:      controlplane.NewAuthClient(cfg, controlPlaneClient),
		Org:       controlplane.NewOrgClient(cfg, controlPlaneClient),
		Billing:   controlplane.NewBillingClient(cfg, controlPlaneClient),
		Audit:     controlplane.NewAuditClient(cfg, controlPlaneClient),
		Events:    publisher,
		Discovery: discovery.NewService(cfg, &http.Client{Timeout: 8 * time.Second}),
		Actions:   actions.NewService(cfg, &http.Client{Timeout: 15 * time.Second}),
		HotPath:   hotpath.NewHTTPWebhookNormalizer(cfg.WebhookHotPathURL, hotPathClient),
		Logger:    &logger,
	})

	errCh := make(chan error, 1)
	go func() {
		logger.Info().Str("port", cfg.Port).Str("service", cfg.ServiceName).Msg("starting integration-corev2")
		errCh <- app.Listen(":" + cfg.Port)
	}()

	stop := make(chan os.Signal, 1)
	signal.Notify(stop, syscall.SIGINT, syscall.SIGTERM)

	select {
	case sig := <-stop:
		logger.Info().Str("signal", sig.String()).Msg("shutdown requested")
	case err := <-errCh:
		if err != nil {
			logger.Fatal().Err(err).Msg("server failed")
		}
	}

	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer shutdownCancel()
	if err := api.Shutdown(shutdownCtx, app); err != nil && !errors.Is(err, context.Canceled) {
		logger.Error().Err(err).Msg("shutdown failed")
	}
	logger.Info().Msg("integration-corev2 stopped")
}

func buildRepository(ctx context.Context, cfg config.Config) (store.Repository, func(), error) {
	if cfg.DatabaseURL == "" {
		repo := store.NewMemoryRepository()
		return repo, repo.Close, nil
	}
	pool, err := db.NewPool(ctx, cfg.DatabaseURL)
	if err != nil {
		return nil, func() {}, err
	}
	if err := db.ApplyMigrations(ctx, pool); err != nil {
		pool.Close()
		return nil, func() {}, err
	}
	repo := store.NewPostgresRepository(pool)
	return repo, repo.Close, nil
}
