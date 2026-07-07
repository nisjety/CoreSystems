package main

import (
	"context"
	"errors"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/triodelab/finspo/internal/api"
	"github.com/triodelab/finspo/internal/config"
	"github.com/triodelab/finspo/internal/content"
	"github.com/triodelab/finspo/internal/dataplane"
	"github.com/triodelab/finspo/internal/db"
	"github.com/triodelab/finspo/internal/events"
	"github.com/triodelab/finspo/internal/sharepoint"
	"github.com/triodelab/finspo/internal/store"
	"github.com/triodelab/finspo/internal/sync"
	"github.com/triodelab/finspo/internal/telemetry"
)

func main() {
	if err := run(); err != nil {
		os.Stderr.WriteString("fatal: " + err.Error() + "\n")
		os.Exit(1)
	}
}

func run() error {
	cfg, err := config.Load()
	if err != nil {
		return err
	}

	tel := telemetry.Init(cfg.ServiceName, cfg.Environment, cfg.OTELExporterEndpoint)
	logger := tel.Logger

	rootCtx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	logger.Info().Str("port", cfg.Port).Msg("starting finspo-core")

	pool, err := db.Open(rootCtx, cfg.DatabaseURL)
	if err != nil {
		return err
	}
	defer pool.Close()
	logger.Info().Msg("postgres pool open")

	if err := db.ApplyMigrations(rootCtx, pool); err != nil {
		return err
	}
	logger.Info().Msg("migrations applied")

	publisher, err := events.Connect(rootCtx, cfg.NATSURL, cfg.ServiceName)
	if err != nil {
		return err
	}
	if cfg.NATSURL == "" {
		logger.Warn().Msg("NATS_URL empty — running with a no-op event publisher")
	} else {
		logger.Info().Str("url", cfg.NATSURL).Msg("nats connected")
	}
	defer func() { _ = publisher.Drain() }()

	st := store.New(pool)
	sourceObjectSink := dataplane.NewSourceObjectClient(cfg.DataPlaneDocumentsURL, cfg.DataPlaneAPIKey)
	tokenProvider := sharepoint.NewHttpAccessTokenProvider(cfg.IntegrationCoreURL, cfg.InternalAPIKey)
	deltaClient := sharepoint.NewDeltaClient(sharepoint.DeltaClientConfig{
		BaseURL:       cfg.GraphBaseURL,
		TokenProvider: tokenProvider,
	})
	permissionsClient := sharepoint.NewPermissionsClient(sharepoint.PermissionsClientConfig{
		BaseURL:       cfg.GraphBaseURL,
		TokenProvider: tokenProvider,
	})
	mutationClient := sharepoint.NewMutationClient(sharepoint.MutationClientConfig{
		BaseURL:       cfg.GraphBaseURL,
		TokenProvider: tokenProvider,
	})

	// Content capture (opt-in): when enabled AND Data Plane documents are
	// configured, download each synced file, extract text, and forward a real
	// content-bearing document to Data Plane v2. Left nil otherwise, so the
	// engine keeps the metadata-only behavior.
	var contentSink sync.ContentSink
	if cfg.CaptureContent && cfg.DataPlaneDocumentsURL != "" && cfg.DataPlaneAPIKey != "" {
		contentClient := sharepoint.NewContentClient(sharepoint.ContentClientConfig{
			BaseURL:       cfg.GraphBaseURL,
			TokenProvider: tokenProvider,
			MaxBytes:      cfg.ContentMaxBytes,
		})
		documentsClient := dataplane.NewDocumentsClient(cfg.DataPlaneDocumentsURL, cfg.DataPlaneAPIKey)
		contentSink = content.NewIngestor(content.Config{
			Fetcher:           contentClient,
			Docs:              documentsClient,
			Logger:            logger,
			ZDRClassification: cfg.ContentZDRClassification,
		})
		logger.Info().Msg("finspo content capture ENABLED: synced files will be forwarded to Data Plane v2 documents")
	}

	subjects := events.NewSubjects(cfg.NATSSubjectPrefix)
	syncEngine := sync.NewEngine(sync.Config{
		Fetcher:            deltaClient,
		Sources:            st.Sources(),
		Items:              st.Items(),
		Cursors:            st.Cursors(),
		Publisher:          publisher,
		Sink:               sourceObjectSink,
		PermissionsFetcher: permissionsClient,
		PermissionsStore:   st.Permissions(),
		CapturePermissions: cfg.CapturePermissions,
		Content:            contentSink,
		Subjects:           subjects,
		Logger:             logger,
		PageLimit:          100,
	})

	executor := sync.NewExecutor(sync.ExecutorConfig{
		Proposals:      st.Proposals(),
		Items:          st.Items(),
		Mutator:        mutationClient,
		Audit:          st.Audit(),
		Publisher:      publisher,
		Subjects:       subjects,
		Logger:         logger,
		AllowExecution: cfg.AllowExecution,
		ArchiveFolder:  cfg.ArchiveFolderID,
	})
	if cfg.AllowExecution {
		logger.Warn().Msg("FINSPO_ALLOW_EXECUTION=true — approved proposals can perform destructive Graph operations")
	}

	scheduler := sync.NewScheduler(sync.SchedulerConfig{
		Sources:  st.Sources(),
		Runner:   syncEngine,
		Locker:   st.Locks(),
		Interval: cfg.SyncInterval,
		Logger:   logger,
	})
	go scheduler.Run(rootCtx)

	app := api.NewServer(api.ServerConfig{
		APIKey:       cfg.APIKey,
		APIKeyHeader: cfg.APIKeyHeader,
		Browser: sharepoint.NewGraphBrowser(sharepoint.GraphBrowserConfig{
			BaseURL:       cfg.GraphBaseURL,
			TokenProvider: tokenProvider,
		}),
		Pool:         pool,
		Publisher:    publisher,
		Logger:       &logger,
		SourceReader: st.Sources(),
		SourceWriter: st.Sources(),
		CursorReader: st.Cursors(),
		SyncRunner:   syncEngine,
		Analytics:    st.Analytics(),
		Proposals:    st.Proposals(),
		Audit:        st.Audit(),
		Recommender:  st.Recommendations(),
		Executor:     executor,
		Subjects:     subjects,
	})

	listenErr := make(chan error, 1)
	go func() {
		if err := app.Listen(":" + cfg.Port); err != nil && !errors.Is(err, http.ErrServerClosed) {
			listenErr <- err
			return
		}
		listenErr <- nil
	}()

	select {
	case <-rootCtx.Done():
		logger.Info().Msg("shutdown signal received")
	case err := <-listenErr:
		if err != nil {
			return err
		}
	}

	shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := app.ShutdownWithContext(shutdownCtx); err != nil {
		logger.Error().Err(err).Msg("server shutdown")
	}
	if err := tel.Shutdown(shutdownCtx); err != nil {
		logger.Error().Err(err).Msg("telemetry shutdown")
	}
	logger.Info().Msg("finspo-core stopped")
	return nil
}
