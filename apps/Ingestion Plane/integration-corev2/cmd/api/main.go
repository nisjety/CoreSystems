package main

import (
	"context"
	"errors"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/integration-corev2/internal/actions"
	"github.com/triodelab/integration-corev2/internal/api"
	"github.com/triodelab/integration-corev2/internal/attestation"
	"github.com/triodelab/integration-corev2/internal/codexsubscription"
	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/controlplane"
	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/db"
	"github.com/triodelab/integration-corev2/internal/discovery"
	"github.com/triodelab/integration-corev2/internal/egress"
	"github.com/triodelab/integration-corev2/internal/events"
	"github.com/triodelab/integration-corev2/internal/handoff"
	"github.com/triodelab/integration-corev2/internal/hotpath"
	"github.com/triodelab/integration-corev2/internal/oauth"
	"github.com/triodelab/integration-corev2/internal/store"
	"github.com/triodelab/integration-corev2/internal/webhookorg"
)

// logLevelFromEnv reads LOG_LEVEL (trace|debug|info|warn|error); anything
// unset or unparseable means INFO.
func logLevelFromEnv() zerolog.Level {
	raw := strings.ToLower(strings.TrimSpace(os.Getenv("LOG_LEVEL")))
	if raw == "" {
		return zerolog.InfoLevel
	}
	level, err := zerolog.ParseLevel(raw)
	if err != nil || level == zerolog.NoLevel {
		return zerolog.InfoLevel
	}
	return level
}

func main() {
	zerolog.TimeFieldFormat = time.RFC3339Nano
	// INFO by default: zerolog's zero value is DEBUG, which made the request
	// logger print the workers' idle claim polls (demoted to DEBUG in
	// api.requestLogLevel) as if nothing had changed. LOG_LEVEL=debug brings
	// them back when tracing the claim loop.
	logger := log.Output(zerolog.ConsoleWriter{Out: os.Stdout, TimeFormat: time.RFC3339}).Level(logLevelFromEnv())

	cfg, err := config.Load()
	if err != nil {
		logger.Fatal().Err(err).Msg("load config")
	}
	if err := cfg.ValidateRuntime(); err != nil {
		logger.Fatal().Err(err).Msg("validate config")
	}
	writeAttestationKeys, err := attestation.ParseTrustedKeysJSON(cfg.ProviderWriteAttestationKeysJSON)
	if err != nil {
		logger.Fatal().Err(err).Msg("initialize provider-write attestation verifier")
	}
	writeAttestations := attestation.NewVerifier(writeAttestationKeys, nil)
	var codexSubscriptions *codexsubscription.Manager
	if cfg.CodexSubscriptionEnabled {
		codexSubscriptions, err = codexsubscription.NewManager(codexsubscription.Config{
			Enabled:           true,
			Home:              cfg.CodexSubscriptionHome,
			LoginTTL:          cfg.CodexSubscriptionLoginTTL,
			InvocationTimeout: cfg.CodexSubscriptionInvocationTimeout,
		}, codexsubscription.NewProcessRunner(cfg.CodexAppServerCommand))
		if err != nil {
			logger.Fatal().Err(err).Msg("initialize Codex subscription broker")
		}
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

	// Microsoft's token/profile endpoints are fixed deployment config
	// (cfg.MicrosoftTokenURL, cfg.MicrosoftGraphBaseURL), but are guarded here
	// purely for consistency with every other provider client
	// oauth.NewProviderClients wires up below via its shared httpClient
	// nil-check — Microsoft is built and injected before that call runs, so
	// it was the one provider client that fell outside that choke point.
	service := oauth.NewService(cfg, repo, vault, oauth.NewMicrosoftClient(oauth.MicrosoftClientConfig{
		ClientID:         cfg.MicrosoftClientID,
		ClientSecret:     cfg.MicrosoftClientSecret,
		ClientAuthMode:   cfg.MicrosoftClientAuthMode,
		TokenOrigin:      cfg.MicrosoftTokenOrigin,
		AuthorizationURL: cfg.MicrosoftAuthorizationURL,
		TokenURL:         cfg.MicrosoftTokenURL,
		GraphBaseURL:     cfg.MicrosoftGraphBaseURL,
		HTTPClient:       egress.SafeClient(egress.ClientConfig{RequestTimeout: 15 * time.Second}),
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
	// Let the OAuth service re-mint Control Plane-owned Microsoft sign-in
	// tokens through auth-core when its own refresh token is absent or dead.
	// NewAuthOAuthClient returns a nil *pointer* when AUTH_CORE_OAUTH_SERVICE_TOKEN
	// is unset; only a non-nil client may be stored in the interface, or the
	// nil-interface check in the service would silently pass.
	if authOAuth := controlplane.NewAuthOAuthClient(cfg, controlPlaneClient); authOAuth != nil {
		service.SetControlPlaneTokenSource(authOAuth)
	}
	hotPathClient := &http.Client{Timeout: 2 * time.Second}
	auditClient := controlplane.NewAuditClient(cfg, controlPlaneClient)
	auditOutbox := api.NewAuditOutbox(repo, auditClient, &logger)
	auditOutbox.Start()
	defer auditOutbox.Close()
	app := api.NewServer(api.ServerConfig{
		Config:      cfg,
		Repo:        repo,
		OAuth:       service,
		Auth:        controlplane.NewAuthClient(cfg, controlPlaneClient),
		Org:         controlplane.NewOrgClient(cfg, controlPlaneClient),
		Billing:     controlplane.NewBillingClient(cfg, controlPlaneClient),
		Audit:       auditClient,
		AuditOutbox: auditOutbox,
		Events:      publisher,
		// Discovery and Actions both dial Shopify hosts assembled from a
		// connection's stored providerContext ("https://"+shop+"/...");
		// egress.SafeClient vets that host instead of trusting net/http's
		// independent second DNS resolution. Every other provider these two
		// services call gets the same connect-timeout/DNS-pinning hardening
		// for free since the client is shared.
		Discovery:          discovery.NewService(cfg, egress.SafeClient(egress.ClientConfig{RequestTimeout: 8 * time.Second})),
		Actions:            actions.NewService(cfg, egress.SafeClient(egress.ClientConfig{RequestTimeout: 15 * time.Second})),
		WriteAttestations:  writeAttestations,
		HotPath:            hotpath.NewHTTPWebhookNormalizer(cfg.WebhookHotPathURL, hotPathClient),
		CodexSubscriptions: codexSubscriptions,
		// Same finspo-core client the finspo worker uses; lets the generic
		// sync route answer 409 no_sources_registered instead of queuing a
		// Microsoft job that has no library to sync. ErrNotConfigured (no
		// FINSPO_API_KEY) falls through to the normal queue path.
		Finspo: handoff.NewFinspoClientFromConfig(cfg, controlPlaneClient),
		WebhookOrg: &webhookorg.Resolver{
			Store: repo,
			Meta: &webhookorg.GraphAssetLister{
				BaseURL:          cfg.FacebookAPIBaseURL,
				InstagramBaseURL: cfg.InstagramAPIBaseURL,
				Tokens:           service,
				// Feeds meta_assets.go's Paging.Next-following calls, which
				// dial a URL taken from Meta's own API response.
				HTTP: egress.SafeClient(egress.ClientConfig{RequestTimeout: 15 * time.Second}),
			},
			Logger: &logger,
		},
		Logger: &logger,
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
