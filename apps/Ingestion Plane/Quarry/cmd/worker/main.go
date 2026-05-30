package main

import (
	"context"
	stdlog "log"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/joho/godotenv"
	"github.com/rs/zerolog"
	zlog "github.com/rs/zerolog/log"
	temporalclient "go.temporal.io/sdk/client"
	"go.temporal.io/sdk/worker"

	"github.com/triodelab/quarry/internal/ai"
	"github.com/triodelab/quarry/internal/api"
	"github.com/triodelab/quarry/internal/config"
	"github.com/triodelab/quarry/internal/modules"
	"github.com/triodelab/quarry/internal/nats"
	"github.com/triodelab/quarry/internal/pipeline"
	"github.com/triodelab/quarry/internal/scraper"
	quarrytemporal "github.com/triodelab/quarry/internal/temporal"
	"github.com/triodelab/quarry/internal/workerqueue"
)

func main() {
	if err := godotenv.Load(); err != nil {
		zlog.Warn().Err(err).Msg("No .env file found (using defaults/environment)")
	}

	// Honor ZEROLOG_LEVEL for local debugging
	if os.Getenv("ZEROLOG_LEVEL") == "debug" {
		zerolog.SetGlobalLevel(zerolog.DebugLevel)
		zlog.Info().Msg("ZEROLOG_LEVEL=debug, zerolog set to Debug")
	}

	cfg, err := config.Load()
	if err != nil {
		stdlog.Fatalf("failed to load config: %v", err)
	}

	if !cfg.TemporalEnabled && cfg.NATSSharedURL == "" {
		zlog.Warn().Msg("No worker backends enabled (set TEMPORAL_ENABLED=true and/or NATS_SHARED_URL)")
		return
	}

	var temporalClientClient temporalclient.Client
	if cfg.TemporalEnabled {
		c, err := quarrytemporal.NewClient(cfg)
		if err != nil {
			stdlog.Fatalf("failed to create temporal client: %v", err)
		}
		temporalClientClient = c
		defer temporalClientClient.Close()
	}

	var aiClient ai.AIClient
	if cfg.EnableAIExtraction {
		restClient, restErr := ai.NewRESTClient(cfg.AICoreHTTPBaseURL, cfg.PublisherNATSURL(), cfg.PublisherNATSToken(), cfg.AICoreInternalAPIKey)
		if restErr != nil {
			stdlog.Fatalf("failed to initialize AI REST client for worker: %v", restErr)
		}
		aiClient = restClient
		zlog.Info().Str("base_url", cfg.AICoreHTTPBaseURL).Msg("AI extraction enabled via Model Plane v2 REST for worker")
	}

	scraperEngine, err := scraper.New(cfg, aiClient)
	if err != nil {
		stdlog.Fatalf("failed to initialize scraper for worker activities: %v", err)
	}
	defer func() {
		if closeErr := scraperEngine.Close(); closeErr != nil {
			zlog.Error().Err(closeErr).Msg("failed to close scraper engine")
		}
	}()

	// ── Build persistent pipeline for StoreResultActivity ────────────────────
	var persister pipeline.ResultPersister
	dsn := cfg.PostgresDSN
	if dsn == "" {
		dsn = os.Getenv("DATABASE_URL")
	}
	if dsn != "" {
		poolCtx, poolCancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer poolCancel()
		pool, poolErr := pgxpool.New(poolCtx, dsn)
		if poolErr == nil {
			if pingErr := pool.Ping(poolCtx); pingErr == nil {
				persister = pipeline.NewJobStorePersister(pool)
				zlog.Info().Msg("worker pipeline: PostgreSQL persister enabled")
			} else {
				pool.Close()
			}
		}
	}
	if persister == nil {
		zlog.Warn().Msg("worker pipeline: no PostgreSQL; results will not be persisted durably")
	}

	workerPipeline := pipeline.NewChain(
		pipeline.NewFingerprintPipeline(),
		pipeline.NewMetadataPipeline(),
		pipeline.NewStatsPipeline(),
		pipeline.NewStoragePipelineWithPersister(persister),
	)

	// ── NATS publisher for cross-plane events from worker ────────────────────
	sharedPub, pubErr := nats.NewSharedPublisher(cfg.PublisherNATSURL(), cfg.PublisherNATSToken(), "quarry-worker")
	if pubErr != nil {
		zlog.Warn().Err(pubErr).Msg("worker: shared NATS publisher not available")
	}

	handler := api.NewHandler(scraperEngine, cfg, sharedPub)
	defer func() {
		if closeErr := handler.Close(); closeErr != nil {
			zlog.Error().Err(closeErr).Msg("failed to close async worker handler")
		}
	}()

	var queue *workerqueue.Queue
	var consumerGroup *workerqueue.ConsumerGroup
	consumerCtx, cancelConsumers := context.WithCancel(context.Background())
	defer cancelConsumers()

	if cfg.NATSSharedURL != "" {
		queue, err = workerqueue.New(cfg.NATSSharedURL, cfg.NATSSharedToken, "quarry-worker-consumer")
		if err != nil {
			stdlog.Fatalf("failed to initialize async worker queue: %v", err)
		}
		if queue != nil {
			consumerGroup, err = queue.StartConsumers(consumerCtx, "quarry-worker", handler.HandleAsyncJob, handler.HandleAsyncCancel)
			if err != nil {
				stdlog.Fatalf("failed to start async worker consumers: %v", err)
			}
			zlog.Info().Msg("async worker queue consumers started")
		}
	}
	defer func() {
		if consumerGroup != nil {
			_ = consumerGroup.Close()
		}
		if queue != nil {
			_ = queue.Close()
		}
	}()

	var temporalWorker worker.Worker
	if cfg.TemporalEnabled {
		if temporalClientClient == nil {
			stdlog.Fatalf("temporal client was not initialized")
		}
		temporalWorker = worker.New(temporalClientClient, cfg.TemporalTaskQueue, worker.Options{})
		activities := quarrytemporal.NewActivitiesWithConfig(scraperEngine, modules.NewDefaultRegistry(), quarrytemporal.ActivitiesConfig{
			Pipeline:  workerPipeline,
			Publisher: sharedPub,
		})

		temporalWorker.RegisterWorkflow(quarrytemporal.CrawlWorkflow)
		temporalWorker.RegisterWorkflow(quarrytemporal.BatchWorkflow)
		temporalWorker.RegisterActivity(activities.FetchPageActivity)
		temporalWorker.RegisterActivity(activities.AnalyzePageActivity)
		temporalWorker.RegisterActivity(activities.StoreResultActivity)

		if err := temporalWorker.Start(); err != nil {
			stdlog.Fatalf("failed to start temporal worker: %v", err)
		}
		zlog.Info().Str("task_queue", cfg.TemporalTaskQueue).Msg("temporal worker started")
	}
	defer func() {
		if temporalWorker != nil {
			temporalWorker.Stop()
		}
	}()

	stop := make(chan os.Signal, 1)
	signal.Notify(stop, os.Interrupt, syscall.SIGTERM)
	<-stop

	cancelConsumers()
}
