// Command api is anbud-core's HTTP service.
package main

import (
	"context"
	"errors"
	"fmt"
	"github.com/triodelab/studio-plane/planeclient"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/anbud/internal/api"
	"github.com/triodelab/anbud/internal/config"
	"github.com/triodelab/anbud/internal/demo"
	"github.com/triodelab/anbud/internal/docread"
	"github.com/triodelab/anbud/internal/docstore"
	"github.com/triodelab/anbud/internal/orgcore"
	"github.com/triodelab/anbud/internal/rawstore"
	"github.com/triodelab/anbud/internal/sources/doffin"
	"github.com/triodelab/anbud/internal/telemetry"
	"github.com/triodelab/anbud/internal/tenderdocs"
	"github.com/triodelab/anbud/internal/workspace"
)

// purgeEvery is how often uploads that have expired are removed. An expired upload is gone to every
// read the moment it expires, so this only decides how long its row lingers.
const purgeEvery = time.Hour

func main() {
	// This binary is also the reader of PDFs: tender documents are from strangers, and the library that
	// reads a PDF can neither be stopped nor bounded from inside the process, so the service runs
	// itself, with nothing of its environment, to read one (internal/docread).
	if code, ok := runAsPDFChild(os.Args); ok {
		os.Exit(code)
	}
	if len(os.Args) == 2 && os.Args[1] == "--migrate-only" {
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
		defer cancel()
		if err := migrateOnlyWith(ctx, os.Getenv, realDatabaseSteps()); err != nil {
			fmt.Fprintln(os.Stderr, "fatal: "+err.Error())
			os.Exit(1)
		}
		return
	}
	if len(os.Args) != 1 {
		fmt.Fprintln(os.Stderr, "fatal: unsupported arguments")
		os.Exit(1)
	}
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "fatal: "+err.Error())
		os.Exit(1)
	}
}

func run() error {
	cfg, err := config.Load()
	if err != nil {
		return err
	}
	planes, err := planeclient.New(planeclient.ConfigFromEnvironment("anbud-core"), nil)
	if err != nil {
		return errors.New("anbud-core cross-plane configuration invalid")
	}

	tel := telemetry.Init(cfg.ServiceName, cfg.Environment, cfg.OTELExporterEndpoint)
	logger := tel.Logger

	rootCtx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	logger.Info().Str("port", cfg.Port).Msg("starting anbud-core")

	// Validate the source settings before touching the database, so a malformed
	// key or base URL stops the service at once and says what to check.
	sources, err := buildSources(cfg, logger)
	if err != nil {
		return err
	}

	pool, err := openAPIDatabaseWith(rootCtx, cfg, realDatabaseSteps())
	if err != nil {
		return err
	}
	defer pool.Close()
	logger.Info().Msg("database ready")

	notices := rawstore.New(pool)
	tender, err := buildTenderDocs(cfg, docstore.New(pool), notices, logger)
	if err != nil {
		return err
	}
	// An interface that holds a nil pointer is not nil: each is set only when the feature is on.
	var documents api.Documents
	var evidence demo.EvidenceSource
	if tender != nil {
		documents, evidence = tender, tender
		go tender.RunPurge(rootCtx, purgeEvery)
	}

	demoService, err := buildDemo(cfg, notices, evidence, logger, workspace.New(pool))
	if err != nil {
		return err
	}

	// Prepare the public read model before accepting searches. No partial corpus
	// is advertised as a complete catalogue during the first backfill.
	if demoService != nil {
		prepareCtx, cancel := context.WithTimeout(rootCtx, 5*time.Minute)
		err := demoService.PrepareCatalogue(prepareCtx)
		if err == nil {
			_, err = demoService.Catalogue(prepareCtx, demo.CatalogueRequest{})
		}
		cancel()
		if err != nil {
			return fmt.Errorf("prepare catalogue: %w", err)
		}
		go demoService.RunCataloguePreparation(rootCtx)
	}

	// A profile suggestion reads the awards the demo service scans. Without the demo service there is no suggestion.
	var wins api.WinSource
	if demoService != nil {
		wins = demoService
	}

	app := api.NewServer(api.ServerConfig{
		APIKey:       cfg.APIKey,
		APIKeyHeader: cfg.APIKeyHeader,
		DB:           pool,
		Sources:      sources,
		Logger:       &logger,
		Demo:         demoService,
		Documents:    documents,
		Workspace:    workspace.New(pool),
		Wins:         wins,
		Planes:       planes,
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
	logger.Info().Msg("anbud-core stopped")
	return nil
}

// buildSources validates the source configuration and returns the status
// provider the HTTP layer reports from. A missing Doffin key is a warning, not
// an error: the plane must come up before the key has been pasted in. A key
// that is present but malformed is a hard error, so a typo never degrades
// silently.
func buildSources(cfg config.Config, logger zerolog.Logger) (func() []api.SourceStatus, error) {
	status := api.SourceStatus{
		ID:         "doffin",
		Configured: cfg.Doffin.Configured(),
		Budgets:    map[string]int{"search": cfg.Doffin.SearchRPM, "download": cfg.Doffin.DownloadRPM},
	}

	if !cfg.Doffin.Configured() {
		logger.Warn().Msg("DOFFIN_API_KEY is not set: Doffin ingestion is disabled and the source reports not_configured")
	} else {
		// The scheduler that consumes this client arrives with the raw-document
		// store; building it here validates the key shape, base URL and budgets
		// at startup and reports the effective budgets.
		client, err := doffin.New(doffin.Config{
			BaseURL:     cfg.Doffin.BaseURL,
			APIKey:      cfg.Doffin.APIKey,
			SearchRPM:   cfg.Doffin.SearchRPM,
			DownloadRPM: cfg.Doffin.DownloadRPM,
			Timeout:     cfg.Doffin.HTTPTimeout,
		})
		if err != nil {
			return nil, fmt.Errorf("doffin source: %w (check DOFFIN_API_KEY and DOFFIN_BASE_URL)", err)
		}
		budgets := client.Budgets()
		status.Budgets = map[string]int{"search": budgets.SearchRPM, "download": budgets.DownloadRPM}
		logger.Info().
			Int("search_rpm", budgets.SearchRPM).
			Int("download_rpm", budgets.DownloadRPM).
			Msg("doffin source configured")
	}

	return func() []api.SourceStatus { return []api.SourceStatus{status} }, nil
}

// runAsPDFChild runs the PDF reader, and reports true, when the process was started as its child: with
// docread.ChildArg as its first argument. The code is the process's exit code.
func runAsPDFChild(args []string) (int, bool) {
	if len(args) > 1 && args[1] == docread.ChildArg {
		return docread.ChildMain(), true
	}
	return 0, false
}

// buildTenderDocs wires the upload of tender documents when ANBUD_TENDER_DOCS is on, and returns nil
// when it is not: the feature keeps a company's own documents, so it is opt-in. PDFs are read by this
// binary run as a child process with an empty environment; the other kinds are read in process.
func buildTenderDocs(cfg config.Config, store tenderdocs.Store, notices tenderdocs.Notices, logger zerolog.Logger) (*tenderdocs.Service, error) {
	if !cfg.TenderDocs.Enabled {
		logger.Info().Msg("ANBUD_TENDER_DOCS is not set: uploads of tender documents are off")
		return nil, nil
	}
	exe, err := os.Executable()
	if err != nil {
		return nil, fmt.Errorf("tender documents: the PDF reader runs this binary, and its path is unknown: %w", err)
	}
	svc, err := tenderdocs.New(tenderdocs.Options{
		Store:     store,
		Notices:   notices,
		Reader:    docread.NewReader(docread.NewChildPDF(exe, []string{docread.ChildArg}), docread.DefaultLimits),
		Limits:    tenderdocs.DefaultLimits(cfg.TenderDocs.MaxArchiveBytes),
		Retention: cfg.TenderDocs.Retention,
		Logger:    &logger,
	})
	if err != nil {
		return nil, fmt.Errorf("tender documents: %w (check ANBUD_TENDER_DOCS_RETENTION and ANBUD_TENDER_DOCS_MAX_MB)", err)
	}
	logger.Info().
		Dur("retention", cfg.TenderDocs.Retention).
		Int64("max_archive_mb", cfg.TenderDocs.MaxArchiveBytes>>20).
		Msg("uploads of tender documents are on")
	return svc, nil
}

// buildDemo wires the demo service: the raw store for notices, when ANBUD_ORG_CORE_URL and
// ANBUD_ORG_CORE_TOKEN are set, org-core for organisation lookups, and, when uploads of tender
// documents are on, what an organisation's uploaded documents say. Without org-core the demo still
// runs, and a profile that names a company is reported as not checked in the register.
func buildDemo(cfg config.Config, notices demo.NoticeSource, evidence demo.EvidenceSource, logger zerolog.Logger, tracking ...demo.TrackingSource) (*demo.Service, error) {
	opts := demo.Options{Notices: notices, Evidence: evidence, Logger: &logger}
	if len(tracking) > 0 {
		opts.Tracking = tracking[0]
	}
	if cfg.OrgCore.Configured() {
		client, err := orgcore.New(orgcore.Config{BaseURL: cfg.OrgCore.URL, Token: cfg.OrgCore.Token})
		if err != nil {
			return nil, fmt.Errorf("org-core lookup: %w (check ANBUD_ORG_CORE_URL and ANBUD_ORG_CORE_TOKEN)", err)
		}
		opts.Companies = client
		logger.Info().Msg("org-core lookup configured")
	} else {
		logger.Warn().Msg("ANBUD_ORG_CORE_URL and ANBUD_ORG_CORE_TOKEN are not set: organisation lookup is disabled")
	}
	return demo.New(opts)
}
