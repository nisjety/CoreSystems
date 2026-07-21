package provisioner

import (
	"context"
	"errors"
	"reflect"
	"strings"
	"testing"
	"time"

	server "github.com/nats-io/nats-server/v2/server"
	"github.com/nats-io/nats.go"
)

func TestParseBusesRequiresDistinctScopedCredentials(t *testing.T) {
	buses, err := ParseBuses(`[{"name":"control","plane":"control","url":"nats://control:4222","user":"observability-provisioner-control","password":"0123456789abcdef0123456789abcdef"},{"name":"model","plane":"model","url":"nats://model:4222","user":"observability-provisioner-model","password":"abcdef0123456789abcdef0123456789"}]`)
	if err != nil {
		t.Fatal(err)
	}
	if len(buses) != 2 || buses[1].Plane != "model" {
		t.Fatalf("buses = %+v", buses)
	}

	for _, raw := range []string{
		`[{"name":"control","plane":"control","url":"nats://control:4222","user":"observability-provisioner-control","password":"short"}]`,
		`[{"name":"control","plane":"control","url":"nats://user:pass@control:4222","user":"observability-provisioner-control","password":"0123456789abcdef0123456789abcdef"}]`,
		`[{"name":"control","plane":"control","url":"nats://control:4222","user":"observability-provisioner-control","password":"0123456789abcdef0123456789abcdef"},{"name":"model","plane":"model","url":"nats://model:4222","user":"observability-provisioner-model","password":"0123456789abcdef0123456789abcdef"}]`,
	} {
		if _, err := ParseBuses(raw); err == nil {
			t.Fatalf("unsafe provisioner configuration accepted: %s", raw)
		}
	}
}

func TestParseBusesRejectsMalformedAndAmbiguousTopology(t *testing.T) {
	t.Parallel()

	validPassword := "0123456789abcdef0123456789abcdef"
	tooMany := make([]string, 9)
	for index := range tooMany {
		tooMany[index] = `{"name":"bus-` + string(rune('a'+index)) + `","plane":"control","url":"nats://control:4222","user":"provisioner","password":"` + strings.Repeat(string(rune('a'+index)), 32) + `"}`
	}
	for name, raw := range map[string]string{
		"malformed":      `{`,
		"empty":          `[]`,
		"too-many":       `[` + strings.Join(tooMany, ",") + `]`,
		"unknown-field":  `[{"name":"control","plane":"control","url":"nats://control:4222","user":"provisioner","password":"` + validPassword + `","token":"forbidden"}]`,
		"invalid-name":   `[{"name":"Control","plane":"control","url":"nats://control:4222","user":"provisioner","password":"` + validPassword + `"}]`,
		"missing-user":   `[{"name":"control","plane":"control","url":"nats://control:4222","password":"` + validPassword + `"}]`,
		"unsafe-scheme":  `[{"name":"control","plane":"control","url":"http://control:4222","user":"provisioner","password":"` + validPassword + `"}]`,
		"unsafe-query":   `[{"name":"control","plane":"control","url":"nats://control:4222?token=secret","user":"provisioner","password":"` + validPassword + `"}]`,
		"duplicate-name": `[{"name":"control","plane":"control","url":"nats://control:4222","user":"one","password":"` + validPassword + `"},{"name":"control","plane":"model","url":"nats://model:4222","user":"two","password":"abcdef0123456789abcdef0123456789"}]`,
	} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			if _, err := ParseBuses(raw); err == nil {
				t.Fatal("unsafe topology was accepted")
			}
		})
	}
}

func TestProvisioningEntrypointsFailFastWhenContextIsCanceled(t *testing.T) {
	t.Parallel()

	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	var js nats.JetStreamContext
	checks := map[string]func() error{
		"plane":       func() error { return Provision(ctx, js, Bus{Name: "control", Plane: "control"}) },
		"model":       func() error { return ProvisionModelRuntime(ctx, js) },
		"application": func() error { return ProvisionApplicationRuntime(ctx, js) },
		"shared":      func() error { return ProvisionControlSharedRuntime(ctx, js) },
		"control":     func() error { return ProvisionControlPlaneRuntime(ctx, js) },
	}
	for name, check := range checks {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			if err := check(); !errors.Is(err, context.Canceled) {
				t.Fatalf("error = %v; want context canceled", err)
			}
		})
	}
}

func TestProvisionIsIdempotentAndPreservesLegacyConsumer(t *testing.T) {
	natsServer, err := server.NewServer(&server.Options{JetStream: true, StoreDir: t.TempDir(), Port: -1})
	if err != nil {
		t.Fatal(err)
	}
	go natsServer.Start()
	if !natsServer.ReadyForConnections(10 * time.Second) {
		t.Fatal("NATS server did not become ready")
	}
	t.Cleanup(func() {
		natsServer.Shutdown()
		natsServer.WaitForShutdown()
	})

	nc, err := nats.Connect(natsServer.ClientURL())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(nc.Close)
	js, err := nc.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := js.AddStream(&nats.StreamConfig{
		Name:     StreamName,
		Subjects: []string{"velion.audit.v1.>", "velion.usage.v1.>", DLQSubject},
		Storage:  nats.FileStorage,
	}); err != nil {
		t.Fatal(err)
	}
	legacy := "audit-core-primary-audit"
	if _, err := js.AddConsumer(StreamName, &nats.ConsumerConfig{
		Durable:        legacy,
		DeliverSubject: "_INBOX.LEGACY",
		DeliverGroup:   legacy,
		FilterSubject:  "velion.audit.v1.>",
		AckPolicy:      nats.AckExplicitPolicy,
	}); err != nil {
		t.Fatal(err)
	}

	bus := Bus{Name: "primary", Plane: "control"}
	if err := Provision(context.Background(), js, bus); err != nil {
		t.Fatalf("first provision: %v", err)
	}
	if err := Provision(context.Background(), js, bus); err != nil {
		t.Fatalf("idempotent provision: %v", err)
	}

	info, err := js.StreamInfo(StreamName)
	if err != nil {
		t.Fatal(err)
	}
	wantSubjects := []string{
		"velion.audit.v1.control.>", "velion.usage.v1.control.>",
		"velion.audit.v2.control.>", "velion.usage.v2.control.>",
		DLQSubject,
	}
	if !reflect.DeepEqual(info.Config.Subjects, wantSubjects) {
		t.Fatalf("stream subjects = %v; want %v", info.Config.Subjects, wantSubjects)
	}
	if _, err := js.ConsumerInfo(StreamName, legacy); err != nil {
		t.Fatalf("legacy consumer was removed: %v", err)
	}
	for _, kind := range []string{"audit", "usage"} {
		if got := ConsumerName(bus.Name, kind); got != "audit-core-primary-v3-"+kind {
			t.Fatalf("v2 authority consumer name = %q", got)
		}
		consumer := ConsumerName(bus.Name, kind)
		consumerInfo, err := js.ConsumerInfo(StreamName, consumer)
		if err != nil {
			t.Fatalf("new %s consumer missing: %v", kind, err)
		}
		if consumerInfo.Config.FilterSubject != PlaneSubject(kind, bus.Plane) ||
			consumerInfo.Config.DeliverSubject != DeliverySubject(bus.Plane, kind) ||
			consumerInfo.Config.DeliverGroup != consumer {
			t.Fatalf("unexpected %s consumer config: %+v", kind, consumerInfo.Config)
		}
	}
}

func TestProvisionControlPlaneRuntimeResourcesIsIdempotent(t *testing.T) {
	natsServer, err := server.NewServer(&server.Options{JetStream: true, StoreDir: t.TempDir(), Port: -1})
	if err != nil {
		t.Fatal(err)
	}
	go natsServer.Start()
	if !natsServer.ReadyForConnections(10 * time.Second) {
		t.Fatal("NATS server did not become ready")
	}
	t.Cleanup(func() {
		natsServer.Shutdown()
		natsServer.WaitForShutdown()
	})

	nc, err := nats.Connect(natsServer.ClientURL())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(nc.Close)
	js, err := nc.JetStream()
	if err != nil {
		t.Fatal(err)
	}

	for run := 0; run < 2; run++ {
		if err := ProvisionControlPlaneRuntime(context.Background(), js); err != nil {
			t.Fatalf("provision run %d: %v", run+1, err)
		}
	}
	for stream, subjects := range map[string][]string{
		AuthEventsStreamName:    {"auth.>"},
		ControlEventsStreamName: {"user.>", "organization.>", "session.created", "session.ended", "billing.>", "usage.>"},
	} {
		info, infoErr := js.StreamInfo(stream)
		if infoErr != nil {
			t.Fatalf("stream %s missing: %v", stream, infoErr)
		}
		if !reflect.DeepEqual(info.Config.Subjects, subjects) {
			t.Fatalf("stream %s subjects = %v; want %v", stream, info.Config.Subjects, subjects)
		}
	}
	consumer, err := js.ConsumerInfo(ControlEventsStreamName, BillingPlanConsumerName)
	if err != nil {
		t.Fatal(err)
	}
	if consumer.Config.FilterSubject != BillingPlanSubject ||
		consumer.Config.DeliverSubject != BillingPlanDeliverySubject ||
		consumer.Config.DeliverGroup != BillingPlanConsumerName ||
		consumer.Config.MaxDeliver != 5 {
		t.Fatalf("unexpected billing plan consumer: %+v", consumer.Config)
	}
}

func TestProvisionControlSharedRuntimeIsIdempotent(t *testing.T) {
	natsServer, err := server.NewServer(&server.Options{JetStream: true, StoreDir: t.TempDir(), Port: -1})
	if err != nil {
		t.Fatal(err)
	}
	go natsServer.Start()
	if !natsServer.ReadyForConnections(10 * time.Second) {
		t.Fatal("NATS server did not become ready")
	}
	t.Cleanup(func() { natsServer.Shutdown(); natsServer.WaitForShutdown() })
	nc, err := nats.Connect(natsServer.ClientURL())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(nc.Close)
	js, err := nc.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	for run := 0; run < 2; run++ {
		if err := Provision(context.Background(), js, Bus{Name: "shared-control", Plane: "shared"}); err != nil {
			t.Fatalf("shared provision run %d: %v", run+1, err)
		}
	}
	info, err := js.StreamInfo(ControlSharedStreamName)
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(info.Config.Subjects, []string{
		"aqencia.controlplane.>", "notifications.>", "velion.session.>",
		"aqencia.reasoning.session.>", "velion.agent.>",
		"aqencia.reasoning.run.>", "app.session.>", ConvexControlDLQSubject,
		GDPRErasureRequestedSubject, GDPRErasureDLQSubject, GDPROwnershipTransferredSubject,
		DocumentsOrgPurgeDLQSubject, OrgDeletionSubjectWildcard,
	}) {
		t.Fatalf("shared subjects = %v", info.Config.Subjects)
	}
	consumer, err := js.ConsumerInfo(ControlSharedStreamName, LegacyBridgeConsumerName)
	if err != nil {
		t.Fatal(err)
	}
	if consumer.Config.FilterSubject != ">" || consumer.Config.MaxDeliver != 20 {
		t.Fatalf("unexpected bridge consumer: %+v", consumer.Config)
	}
	for _, wanted := range convexControlConsumerConfigs() {
		consumer, err := js.ConsumerInfo(ControlSharedStreamName, wanted.Durable)
		if err != nil {
			t.Fatal(err)
		}
		if consumer.Config.DeliverSubject != wanted.DeliverSubject ||
			consumer.Config.DeliverGroup != wanted.DeliverGroup ||
			consumer.Config.FilterSubject != wanted.FilterSubject ||
			consumer.Config.AckPolicy != nats.AckExplicitPolicy ||
			consumer.Config.AckWait != 30*time.Second ||
			consumer.Config.ReplayPolicy != nats.ReplayInstantPolicy {
			t.Fatalf("unexpected Convex Control consumer: %+v", consumer.Config)
		}
	}
	wantedGDPR := gdprDocumentsConsumerConfig()
	gdprConsumer, err := js.ConsumerInfo(ControlSharedStreamName, wantedGDPR.Durable)
	if err != nil {
		t.Fatal(err)
	}
	if gdprConsumer.Config.DeliverSubject != wantedGDPR.DeliverSubject ||
		gdprConsumer.Config.DeliverGroup != wantedGDPR.DeliverGroup ||
		gdprConsumer.Config.FilterSubject != GDPRErasureRequestedSubject ||
		gdprConsumer.Config.AckPolicy != nats.AckExplicitPolicy ||
		gdprConsumer.Config.MaxDeliver != 20 {
		t.Fatalf("unexpected documents GDPR consumer: %+v", gdprConsumer.Config)
	}
	wantedConversationGDPR := conversationOrgErasureConsumerConfig()
	if wantedConversationGDPR.Durable != ConversationOrgErasureConsumerName {
		t.Fatalf("conversation org-erasure durable = %q, want %q (must match conversation-core-go's orgErasureDurable)",
			wantedConversationGDPR.Durable, ConversationOrgErasureConsumerName)
	}
	conversationGDPRConsumer, err := js.ConsumerInfo(ControlSharedStreamName, wantedConversationGDPR.Durable)
	if err != nil {
		t.Fatal(err)
	}
	if conversationGDPRConsumer.Config.DeliverSubject != wantedConversationGDPR.DeliverSubject ||
		conversationGDPRConsumer.Config.DeliverGroup != wantedConversationGDPR.DeliverGroup ||
		conversationGDPRConsumer.Config.FilterSubject != GDPRErasureRequestedSubject ||
		conversationGDPRConsumer.Config.AckPolicy != nats.AckExplicitPolicy ||
		conversationGDPRConsumer.Config.MaxDeliver != 20 {
		t.Fatalf("unexpected conversation-core org-erasure consumer: %+v", conversationGDPRConsumer.Config)
	}

	wantedDocumentsOrgErasure := documentsOrgErasureConsumerConfig()
	documentsOrgErasureConsumer, err := js.ConsumerInfo(ControlSharedStreamName, wantedDocumentsOrgErasure.Durable)
	if err != nil {
		t.Fatal(err)
	}
	if documentsOrgErasureConsumer.Config.DeliverSubject != wantedDocumentsOrgErasure.DeliverSubject ||
		documentsOrgErasureConsumer.Config.DeliverGroup != wantedDocumentsOrgErasure.DeliverGroup ||
		documentsOrgErasureConsumer.Config.FilterSubject != GDPRErasureRequestedSubject ||
		documentsOrgErasureConsumer.Config.AckPolicy != nats.AckExplicitPolicy ||
		documentsOrgErasureConsumer.Config.MaxDeliver != 20 {
		t.Fatalf("unexpected documents-api org-erasure consumer: %+v", documentsOrgErasureConsumer.Config)
	}

	// sessionGDPRErasureConsumerConfig is a PULL consumer (Model Plane's Rust
	// session-core uses async-nats PullConsumer, no DeliverSubject): confirm it
	// was provisioned with empty DeliverSubject/DeliverGroup, not silently
	// dropped or coerced into a push shape.
	wantedSessionGDPR := sessionGDPRErasureConsumerConfig()
	if wantedSessionGDPR.DeliverSubject != "" || wantedSessionGDPR.DeliverGroup != "" {
		t.Fatalf("session-core GDPR consumer config unexpectedly has push delivery: %+v", wantedSessionGDPR)
	}
	sessionGDPRConsumer, err := js.ConsumerInfo(ControlSharedStreamName, wantedSessionGDPR.Durable)
	if err != nil {
		t.Fatal(err)
	}
	if sessionGDPRConsumer.Config.DeliverSubject != "" || sessionGDPRConsumer.Config.DeliverGroup != "" ||
		sessionGDPRConsumer.Config.FilterSubject != GDPRErasureRequestedSubject ||
		sessionGDPRConsumer.Config.AckPolicy != nats.AckExplicitPolicy ||
		sessionGDPRConsumer.Config.MaxDeliver != 20 {
		t.Fatalf("unexpected session-core GDPR pull consumer: %+v", sessionGDPRConsumer.Config)
	}

	wantedQuarry := quarryControlOrgErasureConsumerConfig()
	quarryConsumer, err := js.ConsumerInfo(ControlSharedStreamName, wantedQuarry.Durable)
	if err != nil {
		t.Fatal(err)
	}
	if quarryConsumer.Config.DeliverSubject != wantedQuarry.DeliverSubject ||
		quarryConsumer.Config.DeliverGroup != wantedQuarry.DeliverGroup ||
		quarryConsumer.Config.FilterSubject != GDPRErasureRequestedSubject ||
		quarryConsumer.Config.AckPolicy != nats.AckExplicitPolicy ||
		quarryConsumer.Config.MaxDeliver != 20 {
		t.Fatalf("unexpected quarry-control org-erasure consumer: %+v", quarryConsumer.Config)
	}

	for _, wanted := range notificationOrgDeletionConsumerConfigs() {
		got, err := js.ConsumerInfo(ControlSharedStreamName, wanted.Durable)
		if err != nil {
			t.Fatalf("notification-core consumer %s missing: %v", wanted.Durable, err)
		}
		if got.Config.DeliverSubject != wanted.DeliverSubject ||
			got.Config.DeliverGroup != wanted.DeliverGroup ||
			got.Config.FilterSubject != wanted.FilterSubject ||
			got.Config.AckPolicy != nats.AckExplicitPolicy ||
			got.Config.MaxDeliver != 20 {
			t.Fatalf("unexpected notification-core org-deletion consumer %s: %+v", wanted.Durable, got.Config)
		}
	}

	// The eight Data Plane v2 / Model Plane GDPR org-erasure consumers added
	// alongside documents-api-gdpr/session-core-gdpr/conversation-core-gdpr/
	// quarry-control-gdpr/notification-core-gdpr above. Four are PULL
	// consumers (no DeliverSubject/DeliverGroup): index-engine-rs,
	// graph-index-rs, retrieval-engine-rs, quickwit-adapter-rs. Four are
	// push/queue consumers (nats.Bind + QueueSubscribe): wiki-store-go,
	// data-quality-go, data-orchestrator-go, cost-core.
	for _, wanted := range []*nats.ConsumerConfig{
		indexEngineOrgErasureConsumerConfig(),
		graphIndexOrgErasureConsumerConfig(),
		retrievalEngineOrgErasureConsumerConfig(),
		quickwitAdapterOrgErasureConsumerConfig(),
	} {
		if wanted.DeliverSubject != "" || wanted.DeliverGroup != "" {
			t.Fatalf("consumer %s config unexpectedly has push delivery: %+v", wanted.Durable, wanted)
		}
		got, err := js.ConsumerInfo(ControlSharedStreamName, wanted.Durable)
		if err != nil {
			t.Fatalf("pull consumer %s missing: %v", wanted.Durable, err)
		}
		if got.Config.DeliverSubject != "" || got.Config.DeliverGroup != "" ||
			got.Config.FilterSubject != GDPRErasureRequestedSubject ||
			got.Config.AckPolicy != nats.AckExplicitPolicy ||
			got.Config.MaxDeliver != 20 {
			t.Fatalf("unexpected pull consumer %s: %+v", wanted.Durable, got.Config)
		}
	}
	for _, wanted := range []*nats.ConsumerConfig{
		wikiStoreOrgErasureConsumerConfig(),
		dataQualityOrgErasureConsumerConfig(),
		dataOrchestratorOrgErasureConsumerConfig(),
		costCoreOrgErasureConsumerConfig(),
	} {
		got, err := js.ConsumerInfo(ControlSharedStreamName, wanted.Durable)
		if err != nil {
			t.Fatalf("push consumer %s missing: %v", wanted.Durable, err)
		}
		if got.Config.DeliverSubject != wanted.DeliverSubject ||
			got.Config.DeliverGroup != wanted.DeliverGroup ||
			got.Config.FilterSubject != GDPRErasureRequestedSubject ||
			got.Config.AckPolicy != nats.AckExplicitPolicy ||
			got.Config.MaxDeliver != 20 {
			t.Fatalf("unexpected push consumer %s: %+v", wanted.Durable, got.Config)
		}
	}
}

func TestProvisionModelRuntimeAndApplicationConsumersIsIdempotent(t *testing.T) {
	natsServer, err := server.NewServer(&server.Options{JetStream: true, StoreDir: t.TempDir(), Port: -1})
	if err != nil {
		t.Fatal(err)
	}
	go natsServer.Start()
	if !natsServer.ReadyForConnections(10 * time.Second) {
		t.Fatal("NATS server did not become ready")
	}
	t.Cleanup(func() { natsServer.Shutdown(); natsServer.WaitForShutdown() })
	nc, err := nats.Connect(natsServer.ClientURL())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(nc.Close)
	js, err := nc.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	for run := 0; run < 2; run++ {
		if err := Provision(context.Background(), js, Bus{Name: "model", Plane: "model"}); err != nil {
			t.Fatalf("model provision run %d: %v", run+1, err)
		}
	}
	for stream, subjects := range map[string][]string{
		ModelToolsStreamName:         {"tools.completions.*"},
		ModelOrchestrationStreamName: {"mp.v1.orchestration.>"},
		ModelRunEventsStreamName:     {"mp.v1.run.*.event"},
	} {
		info, infoErr := js.StreamInfo(stream)
		if infoErr != nil {
			t.Fatalf("stream %s missing: %v", stream, infoErr)
		}
		if !reflect.DeepEqual(info.Config.Subjects, subjects) {
			t.Fatalf("stream %s subjects = %v; want %v", stream, info.Config.Subjects, subjects)
		}
	}
	for stream, consumer := range map[string]string{
		ModelToolsStreamName:         SessionToolsConsumerName,
		ModelOrchestrationStreamName: SessionOrchestrationConsumerName,
		ModelRunEventsStreamName:     InsightRunConsumerName,
	} {
		if _, infoErr := js.ConsumerInfo(stream, consumer); infoErr != nil {
			t.Fatalf("consumer %s on %s missing: %v", consumer, stream, infoErr)
		}
	}
	approval, err := js.ConsumerInfo(ModelOrchestrationStreamName, InsightApprovalConsumerName)
	if err != nil {
		t.Fatal(err)
	}
	if approval.Config.FilterSubject != "mp.v1.orchestration.approval" || approval.Config.DeliverSubject != InsightApprovalDeliverySubject {
		t.Fatalf("unexpected insight approval consumer: %+v", approval.Config)
	}
}

func TestProvisionApplicationRuntimeStreamIsIdempotent(t *testing.T) {
	natsServer, err := server.NewServer(&server.Options{JetStream: true, StoreDir: t.TempDir(), Port: -1})
	if err != nil {
		t.Fatal(err)
	}
	go natsServer.Start()
	if !natsServer.ReadyForConnections(10 * time.Second) {
		t.Fatal("NATS server did not become ready")
	}
	t.Cleanup(func() { natsServer.Shutdown(); natsServer.WaitForShutdown() })
	nc, err := nats.Connect(natsServer.ClientURL())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(nc.Close)
	js, err := nc.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	for run := 0; run < 2; run++ {
		if err := Provision(context.Background(), js, Bus{Name: "application", Plane: "application"}); err != nil {
			t.Fatalf("application provision run %d: %v", run+1, err)
		}
	}
	for stream, subjects := range map[string][]string{
		ApplicationEventsStreamName:    {"velion.application.>"},
		ApplicationModelStreamName:     {"velion.model.>"},
		ApplicationIngestionStreamName: {"velion.ingestion.>"},
	} {
		info, infoErr := js.StreamInfo(stream)
		if infoErr != nil {
			t.Fatalf("stream %s missing: %v", stream, infoErr)
		}
		if !reflect.DeepEqual(info.Config.Subjects, subjects) {
			t.Fatalf("stream %s subjects = %v; want %v", stream, info.Config.Subjects, subjects)
		}
	}
	for stream, consumer := range map[string]string{
		ApplicationEventsStreamName:    ConversationAIActionConsumerName,
		ApplicationModelStreamName:     ConversationModelActionConsumerName,
		ApplicationIngestionStreamName: ConversationWebhookConsumerName,
	} {
		if _, infoErr := js.ConsumerInfo(stream, consumer); infoErr != nil {
			t.Fatalf("consumer %s on %s missing: %v", consumer, stream, infoErr)
		}
	}
	if _, err := js.ConsumerInfo(ApplicationEventsStreamName, InsightMetricConsumerName); err != nil {
		t.Fatalf("Insight metric consumer missing: %v", err)
	}
}
