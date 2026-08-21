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
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/capabilityhealth"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/clients"
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
	// unset CONVERSATION_GDPR_SHARED_NATS_URL disables the privacy consumers
	// (hard org erasure and the draft-only ZDR cleanup), mirroring the tolerant
	// natsClient construction above.
	var sharedNatsClient *appnats.Client
	if cfg.SharedNATSURL != "" {
		sharedNatsClient, err = appnats.NewClient(appnats.Config{
			URL: cfg.SharedNATSURL, User: cfg.SharedNATSUser, Password: cfg.SharedNATSPassword,
			InboxPrefix: "_INBOX.CONVERSATION_CORE_GDPR", Name: cfg.ServiceName + "-gdpr",
		})
		if err != nil {
			log.Printf("conversation-core-go: shared-broker NATS disabled (privacy consumers will not run): %v", err)
			sharedNatsClient = nil
		} else {
			defer sharedNatsClient.Close()
		}
	} else {
		log.Printf("conversation-core-go: CONVERSATION_GDPR_SHARED_NATS_URL unset — privacy consumers disabled")
	}

	repository := conversation.NewRepository(db.Pool)
	// Org Core remains the source of truth for retained-AI and recurrence
	// policy. The typed nil is intentional: its methods fail closed when the
	// service-principal configuration is absent, while the optional corpus
	// builder below remains disabled in that configuration.
	orgCoreClient := clients.NewOrgCoreClient(cfg.OrgCoreBaseURL, cfg.OrgCoreServicePrincipal, cfg.OrgCoreServiceToken)

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
		conversation.WithAIProposalPolicy(orgCoreClient),
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

	// The ticket action operation receipt writes a transactionally coupled
	// conversation_events row. A separate leased dispatcher is the only path
	// that publishes it, preserving an at-least-once recovery path when the
	// service dies after committing the owner effect.
	if publisher != nil {
		ticketOperationOutbox := consumers.NewTicketOperationOutboxDispatcher(
			repository, publisher, cfg.ServiceName+"-ticket-operation-outbox", 5*time.Second,
		)
		ticketOperationOutbox.Start(ctx)
		defer ticketOperationOutbox.Stop()
	}

	// Semantic support-recurrence corpus builder (preview): maintains a
	// bounded per-org ticket-similarity embedding corpus. Disabled — not
	// fatal — when org-core or embedding-engine-rs aren't configured, since
	// this is optional preview infrastructure, not a durable data path.
	embeddingClient := clients.NewEmbeddingClient(cfg.EmbeddingEngineBaseURL)
	if orgCoreClient != nil && embeddingClient != nil {
		corpusBuilder := consumers.NewSupportRecurrenceCorpusBuilder(
			service, orgCoreClient, embeddingClient,
			10*time.Minute, 90*24*time.Hour,
		)
		corpusBuilder.Start(ctx)
		defer corpusBuilder.Stop()
	} else {
		log.Printf("conversation-core-go: support-recurrence corpus builder disabled (org-core or embedding-engine client not configured)")
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

		// Propose leg: model/hook-published verevon.model.action.proposed events
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
			webhookConsumer := consumers.NewWebhookReceivedConsumer(natsClient.JS, integrationClient, service, service)
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
	// verevon.gdpr.erasure.requested (explicit hard-delete AND its 30-day
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
		interactiveRetentionConsumer := consumers.NewInteractiveRetentionConsumer(sharedNatsClient.JS, service)
		if err := interactiveRetentionConsumer.Start(ctx); err != nil {
			log.Printf("conversation-core-go: interactive-retention consumer: %v", err)
		} else {
			defer interactiveRetentionConsumer.Stop()
		}
		// Reactive backstop for the corpus builder above: deletes an org's
		// support-recurrence corpus immediately on ZDR-enable, rather than
		// waiting for the next sweep. Independent durable, so it runs
		// regardless of whether the builder itself is configured.
		recurrencePurgeConsumer := consumers.NewSupportRecurrenceZDRPurgeConsumer(sharedNatsClient.JS, service)
		if err := recurrencePurgeConsumer.Start(ctx); err != nil {
			log.Printf("conversation-core-go: support-recurrence zdr-purge consumer: %v", err)
		} else {
			defer recurrencePurgeConsumer.Stop()
		}
	}

	handler := apphttp.NewHandler(cfg, service)
	handler.SetSupportPolicyReader(orgCoreClient)
	if cfg.ControlRunActionDecisionKeyID != "" {
		ownerGrantDecisionVerifier, err := apphttp.NewOwnerGrantDecisionVerifier(cfg.ControlRunActionDecisionKeyID, cfg.ControlRunActionDecisionPublicKey)
		if err != nil {
			log.Fatalf("conversation-core-go: Control owner grant decision verifier: %v", err)
		}
		handler.SetOwnerGrantDecisionVerifier(ownerGrantDecisionVerifier)
		log.Printf("conversation-core-go: Control owner grant decision verifier configured; owner grant issuance remains personal-Space only")
	}
	if cfg.ExecutionCoreServiceToken != "" {
		runActionDecisionVerifier, err := apphttp.NewRunActionDecisionVerifier(cfg.ControlRunActionDecisionKeyID, cfg.ControlRunActionDecisionPublicKey)
		if err != nil {
			log.Fatalf("conversation-core-go: Control run action decision verifier: %v", err)
		}
		handler.SetRunActionDecisionVerifier(runActionDecisionVerifier)
		currentAuthorityValidator, err := apphttp.NewControlRunActionAuthorityValidator(cfg.ControlRunActionAuthorityURL, cfg.ControlRunActionAuthorityToken, cfg.AllowInsecureControlRunActionAuthorityLoopback)
		if err != nil {
			log.Fatalf("conversation-core-go: Control run action authority validator: %v", err)
		}
		handler.SetRunActionAuthorityValidator(currentAuthorityValidator)
		reservationCoordinator, err := apphttp.NewControlOwnerEffectReservationCoordinator(cfg.ControlOwnerEffectReservationURL, cfg.ControlOwnerEffectReservationToken, cfg.AllowInsecureControlRunActionAuthorityLoopback)
		if err != nil {
			log.Fatalf("conversation-core-go: Control owner-effect reservation coordinator: %v", err)
		}
		handler.SetOwnerEffectReservationCoordinator(reservationCoordinator)
		log.Printf("conversation-core-go: Control ticket decision verifier, current-authority validator, and owner-effect reservation coordinator configured; agent ticket effects still require an active exact owner grant")
	} else {
		log.Printf("conversation-core-go: agent ticket action route remains fail-closed (no execution authority configured)")
	}
	if cfg.OwnerActionHealthReady() {
		reporter, reporterErr := capabilityhealth.New(capabilityhealth.Config{
			CapabilityCoreURL: cfg.CapabilityCoreURL,
			AuthCoreURL:       cfg.CapabilityHealthAuthCoreURL,
			ServiceID:         cfg.CapabilityHealthServiceID,
			Credential:        cfg.CapabilityHealthServiceCredential,
			Interval:          cfg.CapabilityHealthInterval,
		})
		if reporterErr != nil {
			log.Printf("conversation-core-go: owner-action capability health disabled: %v", reporterErr)
		} else {
			go reporter.Start(ctx, log.Printf)
			log.Printf("conversation-core-go: owner-action capability health reporter enabled")
		}
	} else {
		log.Printf("conversation-core-go: owner-action capability health reporter disabled; Control-bound execution lane is incomplete")
	}
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
