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

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/consumers"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/eventing"
	apphttp "github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/http"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/integration"
	appnats "github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/nats"
)

func main() {
	ctx := context.Background()
	cfg, err := config.Load()
	if err != nil {
		log.Fatalf("conversation-core-go: config: %v", err)
	}

	db, err := database.Connect(ctx, cfg.DatabaseURL)
	if err != nil {
		log.Fatalf("conversation-core-go: database: %v", err)
	}
	defer db.Close()

	if err := database.RunMigrations(ctx, db); err != nil {
		log.Fatalf("conversation-core-go: migrations: %v", err)
	}

	var publisher conversation.EventPublisher
	var eventPublisher *eventing.Publisher
	natsClient, err := appnats.NewClient(appnats.Config{
		URL:   cfg.NATSURL,
		Token: cfg.NATSToken,
		Name:  cfg.ServiceName,
	})
	if err != nil {
		log.Printf("conversation-core-go: NATS disabled: %v", err)
	} else {
		defer natsClient.Close()
		eventPublisher = eventing.NewPublisher(natsClient.JS)
		if err := eventPublisher.EnsureStream(); err != nil {
			log.Printf("conversation-core-go: ensure stream: %v", err)
		}
		// The model-proposed subject lives in the model namespace, which the
		// application stream does not cover — ensure its own stream so the
		// propose leg is durable.
		if err := eventPublisher.EnsureModelStream(); err != nil {
			log.Printf("conversation-core-go: ensure model stream: %v", err)
		}
		// integration-corev2 (Ingestion Plane) publishes webhook_received via
		// plain core-NATS Publish, not js.Publish — this stream is what makes
		// those events durably consumable by our JetStream QueueSubscribe.
		if err := eventPublisher.EnsureIngestionStream(); err != nil {
			log.Printf("conversation-core-go: ensure ingestion stream: %v", err)
		}
		publisher = eventPublisher
	}

	repository := conversation.NewRepository(db.Pool)
	service := conversation.NewService(repository, publisher)

	// PR-6 act-leg: outbound client to integration-corev2 for draft.reply sends.
	// Constructed only when configured, so the executor never claims a send it
	// cannot perform (a nil sender disables draft.reply execution honestly).
	var sender consumers.OutboundSender
	var integrationClient *integration.Client
	if cfg.DraftReplySendEnabled() {
		integrationClient = integration.NewClient(cfg.IntegrationBaseURL, cfg.IntegrationInternalKey)
		sender = integrationClient
		log.Printf("conversation-core-go: draft.reply outbound-send enabled via %s", cfg.IntegrationBaseURL)
	} else {
		log.Printf("conversation-core-go: draft.reply outbound-send disabled (INTEGRATION_BASE_URL / key unset)")
	}

	// W4 HITL executor: when a human approves an action, promote the ticket
	// (ticket.classification) or send the reply (draft.reply). Only runs when
	// JetStream is available (publisher set above). Idempotent by action id.
	if natsClient != nil && publisher != nil {
		executor := consumers.NewAIActionExecutor(natsClient.JS, repository, service, publisher, sender)
		if err := executor.Start(ctx); err != nil {
			log.Printf("conversation-core-go: ai-action executor: %v", err)
		} else {
			defer executor.Stop()
		}

		// Propose leg: model/hook-published velion.model.action.proposed events
		// queue suggested actions into the HITL review queue.
		proposedConsumer := consumers.NewModelActionProposedConsumer(natsClient.JS, service)
		if err := proposedConsumer.Start(ctx); err != nil {
			log.Printf("conversation-core-go: model-action-proposed consumer: %v", err)
		} else {
			defer proposedConsumer.Stop()
		}

		// Inbound leg: integration-corev2 webhook_received events (WhatsApp,
		// Messenger) fetched and normalized into stored conversation messages.
		// Reuses the same integration client as draft.reply sends, since both
		// need INTEGRATION_BASE_URL / INTEGRATION_INTERNAL_API_KEY configured.
		if integrationClient != nil {
			webhookConsumer := consumers.NewWebhookReceivedConsumer(natsClient.JS, integrationClient, service)
			if err := webhookConsumer.Start(ctx); err != nil {
				log.Printf("conversation-core-go: webhook-received consumer: %v", err)
			} else {
				defer webhookConsumer.Stop()
			}
		} else {
			log.Printf("conversation-core-go: webhook-received consumer disabled (no integration client)")
		}
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
		log.Printf("conversation-core-go: received %s", sig)
	case err := <-errCh:
		if err != nil && !errors.Is(err, stdhttp.ErrServerClosed) {
			log.Fatalf("conversation-core-go: server: %v", err)
		}
	}

	shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := server.Shutdown(shutdownCtx); err != nil {
		log.Printf("conversation-core-go: shutdown: %v", err)
	}
}
