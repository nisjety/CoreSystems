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
	natsClient, err := appnats.NewClient(appnats.Config{
		URL:   cfg.NATSURL,
		Token: cfg.NATSToken,
		Name:  cfg.ServiceName,
	})
	if err != nil {
		log.Printf("conversation-core-go: NATS disabled: %v", err)
	} else {
		defer natsClient.Close()
		eventPublisher := eventing.NewPublisher(natsClient.JS)
		if err := eventPublisher.EnsureStream(); err != nil {
			log.Printf("conversation-core-go: ensure stream: %v", err)
		}
		publisher = eventPublisher
	}

	repository := conversation.NewRepository(db.Pool)
	service := conversation.NewService(repository, publisher)

	// W4 HITL executor: when a human approves a ticket.classification action,
	// promote the suggested ticket + apply routing. Only runs when JetStream is
	// available (publisher set in the NATS branch above). Idempotent by action id.
	if natsClient != nil && publisher != nil {
		executor := consumers.NewAIActionExecutor(natsClient.JS, repository, service, publisher)
		if err := executor.Start(ctx); err != nil {
			log.Printf("conversation-core-go: ai-action executor: %v", err)
		} else {
			defer executor.Stop()
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
