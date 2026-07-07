package main

import (
	"context"
	"log"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"
	"time"

	"github.com/nats-io/nats.go"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/channels"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/consumers"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/eventing"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/feed"
	httpserver "github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/http"
	natsclient "github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/nats"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/preferences"
	runtimeclient "github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/runtime"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/subscribers"
)

func main() {
	cfg, err := config.Load()
	if err != nil {
		log.Fatalf("load config: %v", err)
	}

	startupCtx, startupCancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer startupCancel()

	db, err := database.Connect(startupCtx, cfg.DatabaseURL)
	if err != nil {
		log.Fatalf("connect database: %v", err)
	}
	defer db.Close()

	if err := database.RunMigrations(startupCtx, db, filepath.Join(".", "migrations")); err != nil {
		log.Fatalf("run migrations: %v", err)
	}

	natsClient, err := natsclient.NewClient(natsclient.Config{
		URL:   cfg.NATSURL,
		Token: cfg.NATSToken,
		Name:  cfg.ServiceName,
	})
	if err != nil {
		log.Fatalf("connect velion nats: %v", err)
	}
	defer func() {
		if err := natsClient.Conn.Drain(); err != nil {
			log.Printf("nats drain error: %v", err)
		}
		natsClient.Conn.Close()
	}()

	// ── Wiring ──────────────────────────────────────────────────────────
	repository := notification.NewRepository(db.Pool)
	publisher := eventing.NewPublisher(natsClient.JS)
	if err := publisher.EnsureStream(); err != nil {
		log.Printf("warning: ensure VELION_APPLICATION stream: %v", err)
	}
	runtime := runtimeclient.NewNovuAdapter(runtimeclient.Config{
		SecretKey: cfg.NovuSecretKey,
		BaseURL:   cfg.NovuBaseURL,
	})

	// U5-2 services. Each is independently nil-tolerant in the handlers.
	feedRepo := feed.NewRepository(db.Pool)
	feedSvc := feed.NewService(feedRepo)

	prefRepo := preferences.NewRepository(db.Pool)
	prefSvc := preferences.NewService(prefRepo, runtime)

	channelsRepo := channels.NewRepository(db.Pool)
	channelsSvc := channels.NewService(channelsRepo)

	subRepo := subscribers.NewRepository(db.Pool)
	subSvc := subscribers.NewService(subRepo, runtime)

	// Service.WithFeed hook: every dispatched notification gets mirrored
	// into the local feed cache so the /notifications endpoints serve the
	// same payload that Novu would (we keep our own read/seen/archived
	// semantics). Best-effort — failures log but don't fail the dispatch.
	notificationService := notification.NewService(
		repository,
		runtime,
		publisher,
		notification.WithFeedSink(func(ctx context.Context, params notification.FeedSinkParams) {
			_, err := feedSvc.Create(ctx, feed.CreateParams{
				ID:                    params.RequestID,
				RecipientID:           params.RecipientID,
				EventType:             params.Type,
				Channel:               feed.ChannelInApp, // V0: every dispatch creates an in-app row
				Title:                 params.Title,
				Body:                  params.Body,
				CtaLabel:              params.CtaLabel,
				CtaHref:               params.CtaHref,
				Payload:               params.Payload,
				ActorID:               params.ActorID,
				ActorName:             params.ActorName,
				ActorEmail:            params.ActorEmail,
				ActorAvatar:           params.ActorAvatar,
				Provider:              params.Provider,
				ProviderTransactionID: params.ProviderTransactionID,
				Source:                params.Source,
			})
			if err != nil {
				log.Printf("notification-core: feed sink write failed for %s: %v", params.RequestID, err)
			}
		}),
		notification.WithSubscriberEnsurer(func(ctx context.Context, recipientID string) {
			if _, err := subSvc.EnsureForRecipient(ctx, recipientID); err != nil {
				log.Printf("notification-core: ensure subscriber failed for %s: %v", recipientID, err)
			}
		}),
	)

	handler := httpserver.NewHandler(cfg, httpserver.HandlerDeps{
		Notifications: notificationService,
		Feed:          feedSvc,
		Preferences:   prefSvc,
		Channels:      channelsSvc,
		Subscribers:   subSvc,
	})
	server := httpserver.NewServer(cfg.HTTPPort, handler, cfg.InternalAPIKey)

	// ── Shared bus consumers ────────────────────────────────────────────
	// G14 + U5-2: subscribe to Control Session events from CP session-core
	// + future auth/billing/org-core consumers go alongside this one.
	sharedNATSClient := natsClient
	if cfg.SharedNATSURL != "" && cfg.SharedNATSURL != cfg.NATSURL {
		sc, err := natsclient.NewClient(natsclient.Config{
			URL:   cfg.SharedNATSURL,
			Token: cfg.SharedNATSToken,
			Name:  cfg.ServiceName + "-shared",
		})
		if err != nil {
			log.Printf("warning: connect shared nats (%s): %v — control-session subscriber will not bind", cfg.SharedNATSURL, err)
			sharedNATSClient = nil
		} else {
			sharedNATSClient = sc
			defer func() {
				if drainErr := sc.Conn.Drain(); drainErr != nil {
					log.Printf("shared nats drain error: %v", drainErr)
				}
				sc.Conn.Close()
			}()
			log.Printf("connected to shared nats at %s", cfg.SharedNATSURL)
		}
	}
	if sharedNATSClient != nil {
		if err := ensureSharedConsumerStream(sharedNATSClient.JS); err != nil {
			log.Printf("shared nats stream setup skipped: %v", err)
		}
	}

	var controlSessionSub *consumers.ControlSessionSubscriber
	if sharedNATSClient != nil {
		controlSessionSub = consumers.NewControlSessionSubscriber(sharedNATSClient.JS, notificationService)
		if err := controlSessionSub.Start(context.Background()); err != nil {
			log.Printf("warning: start control-session subscriber: %v", err)
		}
	}
	defer func() {
		if controlSessionSub != nil {
			controlSessionSub.Stop()
		}
	}()

	// Identity sync consumer: when auth-core publishes user lifecycle
	// events on the shared bus, we update the local subscriber row and
	// fan out the identity to Novu. See consumers/identity_sync.go.
	var identitySub *consumers.IdentitySyncSubscriber
	if sharedNATSClient != nil {
		identitySub = consumers.NewIdentitySyncSubscriber(sharedNATSClient.JS, subSvc)
		if err := identitySub.Start(context.Background()); err != nil {
			log.Printf("warning: start identity-sync subscriber: %v", err)
		}
	}
	defer func() {
		if identitySub != nil {
			identitySub.Stop()
		}
	}()

	// Social publish-job failures (task #26, provider business modules
	// program): social-core's own JetStream stream already covers this
	// subject, so this reuses sharedNATSClient rather than needing a new
	// stream. See consumers/social_publish_failed.go.
	var socialPublishFailedSub *consumers.SocialPublishFailedSubscriber
	if sharedNATSClient != nil {
		socialPublishFailedSub = consumers.NewSocialPublishFailedSubscriber(sharedNATSClient.JS, notificationService)
		if err := socialPublishFailedSub.Start(context.Background()); err != nil {
			log.Printf("warning: start social-publish-failed subscriber: %v", err)
		}
	}
	defer func() {
		if socialPublishFailedSub != nil {
			socialPublishFailedSub.Stop()
		}
	}()

	errCh := make(chan error, 1)
	go func() {
		errCh <- server.Start()
	}()

	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, os.Interrupt, syscall.SIGTERM)

	select {
	case sig := <-sigCh:
		log.Printf("shutdown signal received: %s", sig)
	case srvErr := <-errCh:
		if srvErr != nil {
			log.Printf("server error: %v", srvErr)
		}
	}

	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer shutdownCancel()
	if err := server.Shutdown(shutdownCtx); err != nil {
		log.Printf("shutdown error: %v", err)
	}
}

func ensureSharedConsumerStream(js nats.JetStreamContext) error {
	if js == nil {
		return nil
	}

	const streamName = "VELION_SHARED_CONSUMERS"
	requiredSubjects := []string{
		"auth.user.>",
		"org.member.>",
	}

	info, err := js.StreamInfo(streamName)
	if err != nil {
		_, err = js.AddStream(&nats.StreamConfig{
			Name:      streamName,
			Subjects:  requiredSubjects,
			Retention: nats.LimitsPolicy,
			MaxAge:    14 * 24 * time.Hour,
			MaxMsgs:   100_000,
			Storage:   nats.FileStorage,
		})
		return err
	}

	subjects := append([]string{}, info.Config.Subjects...)
	changed := false
	for _, required := range requiredSubjects {
		found := false
		for _, existing := range subjects {
			if existing == required {
				found = true
				break
			}
		}
		if !found {
			subjects = append(subjects, required)
			changed = true
		}
	}
	if !changed {
		return nil
	}

	cfg := info.Config
	cfg.Subjects = subjects
	_, err = js.UpdateStream(&cfg)
	return err
}
