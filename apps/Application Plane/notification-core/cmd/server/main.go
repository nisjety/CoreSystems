package main

import (
	"context"
	"errors"
	"log"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/channels"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/consumers"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/delegation"
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
		URL: cfg.NATSURL, User: cfg.NATSUser, Password: cfg.NATSPassword,
		InboxPrefix: "_INBOX.APPLICATION_NOTIFICATION", Name: cfg.ServiceName,
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

	// Shared Control-Plane broker client: a SECOND, narrowly-scoped
	// connection (identity "notification-core-gdpr") to control-shared-nats,
	// kept separate from natsClient (the Application-Plane-local broker
	// above) so the cross-plane org-deletion subscriber never shares a
	// credential/permission surface with plane-local traffic. Optional — an
	// unset NOTIFICATION_GDPR_SHARED_NATS_URL disables only the org-deletion
	// subscriber.
	var sharedNatsClient *natsclient.Client
	if cfg.SharedNATSURL != "" {
		sharedNatsClient, err = natsclient.NewClient(natsclient.Config{
			URL: cfg.SharedNATSURL, User: cfg.SharedNATSUser, Password: cfg.SharedNATSPassword,
			InboxPrefix: "_INBOX.NOTIFICATION_CORE_GDPR", Name: cfg.ServiceName + "-gdpr",
		})
		if err != nil {
			log.Printf("notification-core: shared-broker NATS disabled (org-deletion subscriber will not run): %v", err)
			sharedNatsClient = nil
		} else {
			defer func() {
				if err := sharedNatsClient.Conn.Drain(); err != nil {
					log.Printf("notification-core: shared nats drain error: %v", err)
				}
				sharedNatsClient.Conn.Close()
			}()
		}
	} else {
		log.Printf("notification-core: NOTIFICATION_GDPR_SHARED_NATS_URL unset — org-deletion subscriber disabled")
	}

	// ── Wiring ──────────────────────────────────────────────────────────
	repository := notification.NewRepository(db.Pool)
	publisher := eventing.NewPublisher(natsClient.JS)
	runtime, err := runtimeclient.NewNovuAdapter(runtimeclient.Config{
		Mode:      cfg.DeliveryMode,
		SecretKey: cfg.NovuSecretKey,
		BaseURL:   cfg.NovuBaseURL,
	})
	if err != nil {
		log.Fatalf("configure notification runtime: %v", err)
	}

	// U5-2 services. Each is independently nil-tolerant in the handlers.
	feedRepo := feed.NewRepository(db.Pool)
	feedSvc := feed.NewService(feedRepo)

	channelsRepo := channels.NewRepository(db.Pool)
	channelsSvc := channels.NewService(channelsRepo)

	subRepo := subscribers.NewRepository(db.Pool)
	subSvc := subscribers.NewService(subRepo, runtime)

	prefRepo := preferences.NewRepository(db.Pool)
	prefSvc := preferences.NewService(prefRepo, runtime, subSvc)

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
				OrganizationID:        params.OrganizationID,
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
				DeliveryStatus:        params.DeliveryStatus,
				SubmittedAt:           params.SubmittedAt,
				DeliveredAt:           params.DeliveredAt,
			})
			if err != nil {
				log.Printf("notification-core: feed sink write failed for %s: %v", params.RequestID, err)
			}
		}),
		notification.WithRecipientResolver(notification.RecipientResolveFn(func(
			ctx context.Context,
			organizationID string,
			recipient notification.Recipient,
		) (*notification.ResolvedRecipient, error) {
			if recipient.Kind != notification.RecipientKindUser {
				return nil, notification.ErrRecipientNotAuthorized
			}
			providerSubscriberID, err := subSvc.ResolveUser(ctx, organizationID, recipient.ID)
			if errors.Is(err, subscribers.ErrNotFound) {
				return nil, notification.ErrRecipientNotAuthorized
			}
			if err != nil {
				return nil, err
			}
			return &notification.ResolvedRecipient{
				Kind:                 notification.RecipientKindUser,
				ID:                   recipient.ID,
				ProviderSubscriberID: providerSubscriberID,
			}, nil
		})),
		notification.WithPreferenceGate(prefSvc),
	)

	handler := httpserver.NewHandler(cfg, httpserver.HandlerDeps{
		Notifications: notificationService,
		Feed:          feedSvc,
		Preferences:   prefSvc,
		Channels:      channelsSvc,
		Recipients:    subSvc,
	})
	delegationVerifier, err := delegation.NewVerifier(delegation.Config{
		Audience: "notification-core",
		Keys:     cfg.DelegationKeys,
	})
	if err != nil {
		log.Fatalf("configure delegation verifier: %v", err)
	}
	server := httpserver.NewServer(cfg.HTTPPort, handler, delegationVerifier)

	// ── Shared bus consumers ────────────────────────────────────────────
	// Most shared-bus notification consumers (ControlSessionSubscriber,
	// SocialPublishFailedSubscriber, IdentitySyncSubscriber) remain off
	// until workload-signed, revisioned authority events and subject ACLs
	// are deployed — see internal/consumers/control_session.go.
	//
	// OrgDeletionSubscriber is started explicitly: the 30-day GDPR
	// soft-delete/purge flow (org-core's owner-gated hardDeleteOrganization
	// + PurgeDeletedOrganizations cron) needs member-facing pending/
	// reminder/cancelled notifications live now. It carries the same
	// unsigned-shared-bus exposure as the disabled consumers above until
	// that hardening work lands.
	log.Printf("notification-core: unsigned shared-bus consumers intentionally disabled (except org-deletion)")

	// consumerCtx (not startupCtx, which is bounded to 15s) lives for the
	// process lifetime — it's captured by the subscription's message
	// handlers and used on every Accept call for as long as the service
	// runs, not just at startup. consumerCancel is deferred AFTER
	// orgDeletionSubscriber's Stop() below (not here) so that, on shutdown,
	// defers run in the reverse order: cancel first (signalling the
	// background retry goroutine to exit before it can call sub.Start again)
	// and only then Stop — avoiding a Start/Stop race on the subscriber's
	// subscription slice.
	consumerCtx, consumerCancel := context.WithCancel(context.Background())

	// Runs on the dedicated shared-broker client (notification-core-gdpr
	// identity) constructed above — independent of natsClient, since this
	// subscriber never touches the Application-Plane-local broker. Gated on
	// sharedNatsClient being non-nil so an intentionally-unconfigured shared
	// broker never reaches Start() at all.
	if sharedNatsClient != nil {
		orgDeletionSubscriber := consumers.NewOrgDeletionSubscriber(sharedNatsClient.JS, notificationService)
		startOrgDeletionSubscriberWithRetry(consumerCtx, orgDeletionSubscriber)
		defer orgDeletionSubscriber.Stop()
	}
	defer consumerCancel()

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

// orgDeletionSubscriberRetryInterval is how often
// startOrgDeletionSubscriberWithRetry re-attempts binding the org-deletion
// subscriber's three JetStream subscriptions after an initial failure.
const orgDeletionSubscriberRetryInterval = 30 * time.Second

// startOrgDeletionSubscriberWithRetry attempts sub.Start once synchronously.
// A wiring gap (the shared-broker consumers not yet provisioned on
// control-shared-nats, a transient connect failure, etc.) used to be fatal —
// main() called log.Fatalf, crash-looping the entire notification-core
// process on every deploy until the gap was closed. Instead this logs a
// warning and keeps retrying in the background on a fixed interval until it
// succeeds or ctx is cancelled, so a remaining wiring gap degrades
// gracefully (org-deletion notifications simply stay unavailable) instead of
// taking down the HTTP API and every other consumer.
//
// Shape mirrors session-core's supervise_background (Model Plane, Rust,
// src/main.rs): log-warn-and-retry-with-sleep rather than fail startup — the
// same non-fatal posture quarry-control's GDPR org-erasure consumer already
// takes for the same class of "not yet provisioned" bind failure
// (Ingestion Plane, cmd/control/main.go: warn and continue rather than
// os.Exit). Unlike session-core's ever-restarting supervisor, Start binds a
// one-time subscription rather than an ever-running loop, so this stops
// retrying as soon as one attempt succeeds.
func startOrgDeletionSubscriberWithRetry(ctx context.Context, sub *consumers.OrgDeletionSubscriber) {
	if err := sub.Start(ctx); err == nil {
		return
	} else {
		log.Printf("notification-core: org-deletion subscriber failed to start, will retry every %s in the background: %v", orgDeletionSubscriberRetryInterval, err)
	}

	go func() {
		ticker := time.NewTicker(orgDeletionSubscriberRetryInterval)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
				if err := sub.Start(ctx); err != nil {
					log.Printf("notification-core: org-deletion subscriber retry failed: %v", err)
					continue
				}
				log.Printf("notification-core: org-deletion subscriber started successfully after retry")
				return
			}
		}
	}()
}
