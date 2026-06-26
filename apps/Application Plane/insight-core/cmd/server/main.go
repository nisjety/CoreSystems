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

	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/briefs"
	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/consumers"
	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/database"
	apphttp "github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/http"
	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/insights"
	appnats "github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/nats"
)

func main() {
	cfg, err := config.Load()
	if err != nil {
		log.Fatalf("insight-core: config: %v", err)
	}

	connectors := insights.DefaultConnectorSlots(insights.ConnectorSlotOptions{
		GoogleAnalyticsAPIBaseURL:     cfg.GoogleAnalyticsAPIBaseURL,
		GoogleSearchConsoleAPIBaseURL: cfg.GoogleSearchConsoleAPIBaseURL,
		TokenLeaseAudience:            cfg.ConnectorTokenLeaseAudience,
	})

	// W3: durable metric store when DATABASE_URL is set; otherwise the in-memory
	// repo (registry-only — recorded metrics do not survive a restart and the
	// scheduled brief cannot run without it).
	var repository insights.Repository = insights.NewMemoryRepository(connectors)
	if cfg.DatabaseURL != "" {
		ctx := context.Background()
		db, err := database.Connect(ctx, cfg.DatabaseURL)
		if err != nil {
			log.Fatalf("insight-core: database: %v", err)
		}
		defer db.Close()
		if err := database.RunMigrations(ctx, db); err != nil {
			log.Fatalf("insight-core: migrations: %v", err)
		}
		repository = insights.NewPGRepository(db.Pool, connectors)
		log.Printf("insight-core: durable metric store (Postgres) enabled")
	} else {
		log.Printf("insight-core: in-memory metric store (set DATABASE_URL for durable metrics)")
	}
	service := insights.NewService(repository)

	// W3 (PR-5): when the shared application NATS is wired, consume conversation-core
	// AND social-core application events and record them as metrics — two of the real
	// producers behind the metrics view. No-op when NATS_URL is empty.
	if cfg.NATSURL != "" {
		natsClient, err := appnats.NewClient(appnats.Config{URL: cfg.NATSURL, Token: cfg.NATSToken, Name: cfg.ServiceName})
		if err != nil {
			log.Printf("insight-core: NATS disabled: %v", err)
		} else {
			defer natsClient.Close()
			subscriber := consumers.NewMetricSubscriber(natsClient.JS, service)
			if err := subscriber.Start(context.Background()); err != nil {
				log.Printf("insight-core: metric subscriber: %v", err)
			} else {
				defer subscriber.Stop()
			}
		}
	}

	// W3 (PR-5): the model-plane-agents producer leg. The Model Plane runs on an
	// isolated NATS cluster, so the agent subscriber dual-connects via the
	// model-plane-nats bridge and records run/tool/approval lifecycle events as
	// surface=agents metrics — the third real producer. No-op when
	// MODEL_PLANE_NATS_URL is empty.
	if cfg.ModelPlaneNATSURL != "" {
		mpNATS, err := appnats.NewClient(appnats.Config{URL: cfg.ModelPlaneNATSURL, Token: cfg.ModelPlaneNATSToken, Name: cfg.ServiceName + "-agents"})
		if err != nil {
			log.Printf("insight-core: model-plane NATS disabled: %v", err)
		} else {
			defer mpNATS.Close()
			// The Model Plane publishes run lifecycle via core NATS with no stream;
			// provision a bounded one so the durable consumer can bind (idempotent,
			// see nats.Client.EnsureStream). Approval events already have a stream
			// (MP_ORCHESTRATION_EVENTS covers mp.v1.orchestration.>), so we cover
			// ONLY the run subject to avoid a subject overlap.
			if err := mpNATS.EnsureStream("MODEL_PLANE_RUN_EVENTS", []string{
				"mp.v1.run.*.event",
			}); err != nil {
				log.Printf("insight-core: model-plane run-events stream: %v", err)
			}
			agentSub := consumers.NewAgentSubscriber(mpNATS.JS, service)
			if err := agentSub.Start(context.Background()); err != nil {
				log.Printf("insight-core: agent subscriber: %v", err)
			} else {
				defer agentSub.Stop()
				log.Printf("insight-core: model-plane-agents producer enabled")
			}
		}
	}

	// W3 (PR-5): scheduled brief DELIVERY. When notification-core is reachable and
	// a durable metric store is enabled, run the daily-brief scheduler. It discovers
	// active orgs server-side from the recorded metric store (IDOR-clean — never
	// from client input) and POSTs a `daily_brief` notification carrying the Preview
	// gate to notification-core's existing Novu adapter (in_app + email). No-op when
	// NOTIFICATION_CORE_URL is empty or the store is in-memory.
	if cfg.NotificationCoreURL != "" && cfg.DatabaseURL != "" {
		notifier := briefs.NewHTTPNotificationClient(cfg.NotificationCoreURL, cfg.InternalAPIKey)
		scheduler := briefs.NewScheduler(service, notifier)
		schedCtx, cancelSched := context.WithCancel(context.Background())
		defer cancelSched()
		go scheduler.Run(schedCtx)
		log.Printf("insight-core: daily_brief scheduler enabled (delivery via notification-core)")
	} else {
		log.Printf("insight-core: daily_brief scheduler disabled (set NOTIFICATION_CORE_URL + DATABASE_URL to enable)")
	}

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
