// email-worker polls connected Gmail (Google) and Outlook (Microsoft)
// mailboxes — plus Microsoft Teams, Slack, X direct messages, and Discord
// guild channels — for new inbound messages and forwards them to the
// conversation-ingest-rs bridge, which lands them in the Verevon Inbox.
// It is the caller side of the /internal/ingest/email pathway.
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

	"github.com/triodelab/integration-corev2/internal/config"
	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/db"
	"github.com/triodelab/integration-corev2/internal/egress"
	"github.com/triodelab/integration-corev2/internal/emailsync"
	"github.com/triodelab/integration-corev2/internal/handoff"
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
	if err := cfg.ValidateEmailWorkerRuntime(); err != nil {
		logger.Fatal().Err(err).Msg("validate email worker config")
	}

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer cancel()

	pool, err := db.NewPool(ctx, cfg.DatabaseURL)
	if err != nil {
		logger.Fatal().Err(err).Msg("connect database")
	}
	defer pool.Close()
	if err := db.ApplyMigrations(ctx, pool); err != nil {
		logger.Fatal().Err(err).Msg("apply migrations")
	}
	repo := store.NewPostgresRepository(pool)

	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		logger.Fatal().Err(err).Msg("initialize token vault")
	}
	tokens := oauth.NewService(cfg, repo, vault, oauth.NewMicrosoftClient(oauth.MicrosoftClientConfig{
		ClientID:         cfg.MicrosoftClientID,
		ClientSecret:     cfg.MicrosoftClientSecret,
		ClientAuthMode:   cfg.MicrosoftClientAuthMode,
		TokenOrigin:      cfg.MicrosoftTokenOrigin,
		AuthorizationURL: cfg.MicrosoftAuthorizationURL,
		TokenURL:         cfg.MicrosoftTokenURL,
		GraphBaseURL:     cfg.MicrosoftGraphBaseURL,
		HTTPClient:       &http.Client{Timeout: 15 * time.Second},
	}))

	// Shared across every inbound-message provider fetcher below. Graph's
	// delta sync in particular follows @odata.nextLink/@odata.deltaLink
	// pagination URLs taken from Microsoft's own API response (see
	// GraphFetcher.safeNextLink for the same-origin check that runs before
	// one is followed); egress.SafeClient adds the resolve+vet+pin layer
	// underneath that check so a rebound connection for that same origin
	// still lands on a vetted, non-private address. Gmail/Teams/Slack/XDM/
	// Discord get the same connect-timeout hardening for free.
	providerHTTP := egress.SafeClient(egress.ClientConfig{RequestTimeout: 20 * time.Second})
	worker := emailsync.Worker{
		Store:       repo,
		Tokens:      tokens,
		Integration: handoff.NewIntegrationClientFromConfig(cfg, &http.Client{Timeout: 15 * time.Second}),
		Ingest: &emailsync.IngestClient{
			BaseURL:      cfg.ConversationIngestURL,
			ServiceToken: cfg.ConversationIngestServiceToken,
			HTTP:         &http.Client{Timeout: 15 * time.Second},
		},
		Gmail: &emailsync.GmailFetcher{HTTP: providerHTTP},
		Graph: &emailsync.GraphFetcher{
			BaseURL:      strings.TrimRight(cfg.MicrosoftGraphBaseURL, "/") + "/v1.0",
			HTTP:         providerHTTP,
			FullBackfill: cfg.EmailSyncGraphFullBackfill,
		},
		Teams:              &emailsync.TeamsFetcher{BaseURL: strings.TrimRight(cfg.MicrosoftGraphBaseURL, "/") + "/v1.0", HTTP: providerHTTP},
		Slack:              &emailsync.SlackFetcher{BaseURL: cfg.SlackAPIBaseURL, HTTP: providerHTTP},
		XDM:                &emailsync.XDMFetcher{BaseURL: cfg.XAPIBaseURL, HTTP: providerHTTP},
		Discord:            &emailsync.DiscordFetcher{BaseURL: cfg.DiscordAPIBaseURL, BotToken: cfg.DiscordBotToken, HTTP: providerHTTP},
		Logger:             &logger,
		PollInterval:       cfg.EmailSyncInterval,
		ManualPollInterval: 2 * time.Second,
		BackfillWindow:     cfg.EmailSyncBackfillWindow,
		MaxPerCycle:        cfg.EmailSyncMaxPerCycle,
	}

	logger.Info().
		Dur("interval", cfg.EmailSyncInterval).
		Str("ingest_url", cfg.ConversationIngestURL).
		Msg("starting email inbound sync worker")
	if err := worker.Run(ctx); err != nil && !errors.Is(err, context.Canceled) {
		logger.Fatal().Err(err).Msg("email sync worker stopped with error")
	}
	logger.Info().Msg("email sync worker stopped")
}
