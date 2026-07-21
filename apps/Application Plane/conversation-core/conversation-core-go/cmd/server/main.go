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

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/attestation"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/consumers"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/delegation"
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
		URL: cfg.NATSURL, User: cfg.NATSUser, Password: cfg.NATSPassword,
		InboxPrefix: "_INBOX.APPLICATION_CONVERSATION", Name: cfg.ServiceName,
	})
	if err != nil {
		log.Printf("conversation-core-go: NATS disabled: %v", err)
	} else {
		defer natsClient.Close()
		eventPublisher = eventing.NewPublisher(natsClient.JS)
		publisher = eventPublisher
	}

	// Shared Control-Plane broker client: a SECOND, narrowly-scoped
	// connection (identity "conversation-core-gdpr") to control-shared-nats,
	// kept separate from natsClient (the Application-Plane-local broker
	// above) so the cross-plane GDPR org-erasure consumer never shares a
	// credential/permission surface with plane-local traffic. Optional — an
	// unset CONVERSATION_GDPR_SHARED_NATS_URL disables only the org-erasure
	// consumer, mirroring the tolerant natsClient construction above.
	var sharedNatsClient *appnats.Client
	if cfg.SharedNATSURL != "" {
		sharedNatsClient, err = appnats.NewClient(appnats.Config{
			URL: cfg.SharedNATSURL, User: cfg.SharedNATSUser, Password: cfg.SharedNATSPassword,
			InboxPrefix: "_INBOX.CONVERSATION_CORE_GDPR", Name: cfg.ServiceName + "-gdpr",
		})
		if err != nil {
			log.Printf("conversation-core-go: shared-broker NATS disabled (org-erasure consumer will not run): %v", err)
			sharedNatsClient = nil
		} else {
			defer sharedNatsClient.Close()
		}
	} else {
		log.Printf("conversation-core-go: CONVERSATION_GDPR_SHARED_NATS_URL unset — org-erasure consumer disabled")
	}

	repository := conversation.NewRepository(db.Pool)

	// Outbound client to integration-corev2, shared by the draft.reply act-leg
	// AND the human-typed Inbox reply path. Constructed only when configured, so
	// neither the executor nor the reply path ever claims a send it cannot
	// perform (a nil sender disables outbound delivery honestly).
	var sender consumers.OutboundSender
	var integrationClient *integration.Client
	if cfg.DraftReplySendEnabled() {
		writeAttestor, signerErr := attestation.NewSigner(attestation.Config{
			PrivateKey: cfg.AttestationPrivateKey,
			KeyID:      cfg.AttestationKeyID,
			Issuer:     attestation.IssuerConversationCore,
			Audience:   attestation.AudienceIntegrationCore,
			Presenter:  attestation.PresenterConversationCore,
		})
		if signerErr != nil {
			log.Fatalf("conversation-core-go: provider-write attestation signer: %v", signerErr)
		}
		integrationClient = integration.NewClient(
			cfg.IntegrationBaseURL,
			cfg.IntegrationInternalKey,
			integration.WithServicePrincipal(
				cfg.AuthCoreURL,
				cfg.IntegrationServiceID,
				cfg.IntegrationServiceCredential,
			),
			integration.WithWriteAttestor(writeAttestor),
		)
		sender = integrationClient
		log.Printf("conversation-core-go: outbound-send enabled via %s", cfg.IntegrationBaseURL)
	} else {
		log.Printf("conversation-core-go: outbound-send disabled (integration or Auth Core service-principal configuration unset)")
	}

	// The Service delivers human agent replies to channel-backed conversations
	// (whatsapp, messenger, …) through the same integration client, so a "Reply
	// sent" in the Inbox reflects a real delivery. When unconfigured, replies are
	// stored without a false send claim. Only wire the sender when a real client
	// exists — passing a typed-nil would make the Service attempt (and fail) sends.
	// Mirrors every feedback submission into the team's own monitored org so
	// external-pilot-org feedback stays visible (see
	// conversation.Service.mirrorFeedback). Always registered -- an empty
	// FeedbackMirrorOrgID disables mirroring inside the Service itself.
	serviceOpts := []conversation.Option{
		conversation.WithFeedbackMirrorOrgID(cfg.FeedbackMirrorOrgID),
	}
	if cfg.FeedbackMirrorOrgID != "" {
		log.Printf("conversation-core-go: feedback mirroring enabled into org %s", cfg.FeedbackMirrorOrgID)
	} else {
		log.Printf("conversation-core-go: FEEDBACK_MIRROR_ORG_ID unset -- feedback mirroring disabled")
	}
	if integrationClient != nil {
		serviceOpts = append(serviceOpts, conversation.WithSender(integrationClient))
	}
	service := conversation.NewService(repository, publisher, serviceOpts...)

	// Stuck-send sweep for the review-approve-send path: periodically flips any
	// outbound intent that has sat in `sending` past cfg.OutboundReconcileStaleAfter
	// to `unknown` for operator reconciliation (see
	// conversation.Service.ReconcileStaleOutboundIntents). Runs unconditionally —
	// it only ever touches conversation-core's own ledger, never a provider — so
	// it is not gated on NATS or the integration client being configured.
	reconciler := consumers.NewOutboundIntentReconciler(service, cfg.OutboundReconcileInterval, cfg.OutboundReconcileStaleAfter)
	reconciler.Start(ctx)
	defer reconciler.Stop()

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

	// Cross-plane GDPR erasure fan-out: org-core publishes
	// velion.gdpr.erasure.requested (explicit hard-delete AND its 30-day
	// auto-purge cron) on the shared Control-Plane bus; this hard-purges
	// every conversation_* row conversation-core holds for that org. Runs on
	// the dedicated shared-broker client (conversation-core-gdpr identity)
	// constructed above — independent of natsClient/publisher, since this
	// consumer never touches the Application-Plane-local broker.
	if sharedNatsClient != nil {
		orgErasureConsumer := consumers.NewOrgErasureConsumer(sharedNatsClient.JS, service)
		if err := orgErasureConsumer.Start(ctx); err != nil {
			log.Printf("conversation-core-go: org-erasure consumer: %v", err)
		} else {
			defer orgErasureConsumer.Stop()
		}
	}

	handler := apphttp.NewHandler(cfg, service)
	delegationVerifier, err := delegation.NewVerifier(delegation.Config{
		Audience: "conversation-core",
		Keys:     cfg.DelegationKeys,
	})
	if err != nil {
		log.Fatalf("conversation-core-go: delegation verifier: %v", err)
	}
	server := apphttp.NewServer(cfg.HTTPPort, handler, delegationVerifier)

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
