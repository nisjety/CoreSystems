package provisioner

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/url"
	"regexp"
	"strings"
	"time"

	"github.com/nats-io/nats.go"
)

const (
	StreamName                                   = "VELION_CONTROL_OBSERVABILITY"
	DLQSubject                                   = "velion.dlq.audit-core.>"
	AuthEventsStreamName                         = "AUTH_EVENTS"
	ControlEventsStreamName                      = "CONTROL_PLANE_EVENTS"
	BillingPlanConsumerName                      = "billing-core-organization-plan-changed"
	BillingPlanSubject                           = "organization.plan.changed"
	BillingPlanDeliverySubject                   = "_VELION.CONTROL.DELIVER.billing.organization-plan-changed"
	ControlSharedStreamName                      = "AQENCIA_CONTROLPLANE"
	LegacyBridgeConsumerName                     = "control-shared-legacy-bridge"
	LegacyBridgeDelivery                         = "_VELION.CONTROL.SHARED.DELIVER.legacy"
	ConvexControlDLQSubject                      = "velion.application.dlq.convex.controlplane"
	GDPRErasureRequestedSubject                  = "velion.gdpr.erasure.requested"
	GDPRErasureDLQSubject                        = "velion.gdpr.erasure.dlq.documents-api"
	GDPROwnershipTransferredSubject              = "velion.gdpr.ownership.transferred"
	DocumentsGDPRConsumerName                    = "documents-api-gdpr-erasure-v1"
	DocumentsGDPRDeliverySubject                 = "_VELION.CONTROL.SHARED.DELIVER.data.documents-api.gdpr-erasure"
	DocumentsOrgErasureConsumerName              = "documents-api-org-erasure"
	DocumentsOrgErasureDelivery                  = "_VELION.CONTROL.SHARED.DELIVER.data.documents-api.org-erasure"
	DocumentsOrgPurgeDLQSubject                  = "velion.gdpr.erasure.dlq.documents-api-org-purge"
	ConversationOrgErasureConsumerName           = "conversation-core-org-erasure"
	ConversationOrgErasureDelivery               = "_VELION.CONTROL.SHARED.DELIVER.application.conversation.gdpr-erasure"
	SessionGDPRErasureConsumerName               = "session-core-gdpr-erasure-v1"
	QuarryControlOrgErasureConsumerName          = "quarry-control-org-erasure"
	QuarryControlOrgErasureDelivery              = "_VELION.CONTROL.SHARED.DELIVER.ingestion.quarry-control.org-erasure"
	OrgDeletionSubjectWildcard                   = "velion.org.deletion.>"
	OrgDeletionPendingSubject                    = "velion.org.deletion.pending"
	OrgDeletionReminderSubject                   = "velion.org.deletion.reminder"
	OrgDeletionCancelledSubject                  = "velion.org.deletion.cancelled"
	NotificationOrgDeletionPendingConsumerName   = "notification-core-org-deletion-pending"
	NotificationOrgDeletionReminderConsumerName  = "notification-core-org-deletion-reminder"
	NotificationOrgDeletionCancelledConsumerName = "notification-core-org-deletion-cancelled"
	NotificationOrgDeletionPendingDelivery       = "_VELION.CONTROL.SHARED.DELIVER.application.notification.org-deletion-pending"
	NotificationOrgDeletionReminderDelivery      = "_VELION.CONTROL.SHARED.DELIVER.application.notification.org-deletion-reminder"
	NotificationOrgDeletionCancelledDelivery     = "_VELION.CONTROL.SHARED.DELIVER.application.notification.org-deletion-cancelled"
	ModelToolsStreamName                         = "TOOLS_COMPLETIONS"
	ModelOrchestrationStreamName                 = "MP_ORCHESTRATION_EVENTS"
	ModelRunEventsStreamName                     = "MODEL_PLANE_RUN_EVENTS"
	SessionToolsConsumerName                     = "session-core-tools"
	SessionOrchestrationConsumerName             = "session-core-orchestration"
	InsightRunConsumerName                       = "insight-core-agent-run-subscriber"
	InsightApprovalConsumerName                  = "insight-core-agent-approval-subscriber"
	InsightRunDeliverySubject                    = "_VELION.MODEL.DELIVER.application.insight.run"
	InsightApprovalDeliverySubject               = "_VELION.MODEL.DELIVER.application.insight.approval"
	ApplicationEventsStreamName                  = "VELION_APPLICATION"
	ApplicationModelStreamName                   = "VELION_MODEL"
	ApplicationIngestionStreamName               = "VELION_INGESTION"
	ConversationAIActionConsumerName             = "conversation-core-ai-action-executor"
	ConversationModelActionConsumerName          = "conversation-core-model-action-proposed"
	ConversationWebhookConsumerName              = "conversation-core-webhook-received"
	InsightMetricConsumerName                    = "insight-core-metric-subscriber"
	ConversationAIActionDelivery                 = "_VELION.APPLICATION.DELIVER.conversation.ai-action-reviewed"
	ConversationModelActionDelivery              = "_VELION.APPLICATION.DELIVER.conversation.model-action-proposed"
	ConversationWebhookDelivery                  = "_VELION.APPLICATION.DELIVER.conversation.webhook-received"
	InsightMetricDelivery                        = "_VELION.APPLICATION.DELIVER.insight.metrics"
	IndexEngineOrgErasureConsumerName            = "index-engine-org-erasure"
	GraphIndexOrgErasureConsumerName             = "graph-index-gdpr-erasure-v1"
	WikiStoreOrgErasureConsumerName              = "wiki-store-org-erasure"
	WikiStoreOrgErasureDelivery                  = "_VELION.CONTROL.SHARED.DELIVER.data.wiki-store.org-erasure"
	RetrievalEngineOrgErasureConsumerName        = "retrieval-engine-gdpr-erasure-v1"
	DataQualityOrgErasureConsumerName            = "data-quality-org-erasure"
	DataQualityOrgErasureDelivery                = "_VELION.CONTROL.SHARED.DELIVER.data.data-quality.org-erasure"
	DataOrchestratorOrgErasureConsumerName       = "data-orchestrator-org-erasure"
	DataOrchestratorOrgErasureDelivery           = "_VELION.CONTROL.SHARED.DELIVER.data.data-orchestrator.org-erasure"
	QuickwitAdapterOrgErasureConsumerName        = "quickwit-adapter-gdpr-erasure-v1"
	CostCoreOrgErasureConsumerName               = "cost-core-org-erasure"
	CostCoreOrgErasureDelivery                   = "_VELION.CONTROL.SHARED.DELIVER.model.cost-core.org-erasure"
	EmbeddingEngineOrgErasureConsumerName        = "embedding-engine-org-erasure"
)

var identifierPattern = regexp.MustCompile(`^[a-z][a-z0-9-]{0,31}$`)

// Bus is a deployment-only broker target. Runtime audit-core never receives
// these credentials and therefore cannot mutate streams or consumers.
type Bus struct {
	Name     string `json:"name"`
	Plane    string `json:"plane"`
	URL      string `json:"url"`
	User     string `json:"user"`
	Password string `json:"password"`
	// Optional marks an external-plane bus whose outage must NOT fail this
	// stack: if provisioning it fails (e.g. the plane is down), the provisioner
	// logs a warning and continues rather than exiting non-zero (which would
	// crash-loop under `restart: on-failure`).
	Optional bool `json:"optional,omitempty"`
}

func ParseBuses(raw string) ([]Bus, error) {
	decoder := json.NewDecoder(strings.NewReader(raw))
	decoder.DisallowUnknownFields()
	var buses []Bus
	if err := decoder.Decode(&buses); err != nil {
		return nil, fmt.Errorf("decode provisioner buses: %w", err)
	}
	if len(buses) == 0 || len(buses) > 8 {
		return nil, errors.New("provisioner buses must contain between 1 and 8 entries")
	}
	seenNames := make(map[string]struct{}, len(buses))
	seenPasswords := make(map[string]struct{}, len(buses))
	for index, bus := range buses {
		if !identifierPattern.MatchString(bus.Name) || !identifierPattern.MatchString(bus.Plane) {
			return nil, fmt.Errorf("bus %d has invalid name or plane", index)
		}
		if bus.User == "" || len(bus.Password) < 32 {
			return nil, fmt.Errorf("bus %q requires a scoped user and a password of at least 32 characters", bus.Name)
		}
		parsed, err := url.Parse(bus.URL)
		if err != nil || parsed.Scheme != "nats" || parsed.Host == "" || parsed.User != nil || parsed.RawQuery != "" || parsed.Fragment != "" || (parsed.Path != "" && parsed.Path != "/") {
			return nil, fmt.Errorf("bus %q has an unsafe NATS URL", bus.Name)
		}
		if _, exists := seenNames[bus.Name]; exists {
			return nil, fmt.Errorf("duplicate bus name %q", bus.Name)
		}
		if _, exists := seenPasswords[bus.Password]; exists {
			return nil, fmt.Errorf("bus %q reuses a provisioner password", bus.Name)
		}
		seenNames[bus.Name] = struct{}{}
		seenPasswords[bus.Password] = struct{}{}
	}
	return buses, nil
}

func PlaneSubject(kind, plane string) string {
	return fmt.Sprintf("velion.%s.v2.%s.>", kind, plane)
}

func LegacyPlaneSubject(kind, plane string) string {
	return fmt.Sprintf("velion.%s.v1.%s.>", kind, plane)
}

func ConsumerName(busName, kind string) string {
	return fmt.Sprintf("audit-core-%s-v3-%s", busName, kind)
}

func DeliverySubject(plane, kind string) string {
	return fmt.Sprintf("_VELION.AUDIT.DELIVER.%s.%s-v2", plane, kind)
}

// Provision converges only the known observability resources. It never
// deletes a stream, consumer, or message; legacy consumers remain available
// for rollback until a separately approved cleanup.
func Provision(ctx context.Context, js nats.JetStreamContext, bus Bus) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if bus.Plane == "shared" {
		return ProvisionControlSharedRuntime(ctx, js)
	}
	wantedSubjects := []string{
		LegacyPlaneSubject("audit", bus.Plane),
		LegacyPlaneSubject("usage", bus.Plane),
		PlaneSubject("audit", bus.Plane),
		PlaneSubject("usage", bus.Plane),
		DLQSubject,
	}
	streamInfo, err := js.StreamInfo(StreamName)
	switch {
	case errors.Is(err, nats.ErrStreamNotFound):
		_, err = js.AddStream(&nats.StreamConfig{
			Name:      StreamName,
			Subjects:  wantedSubjects,
			Storage:   nats.FileStorage,
			Retention: nats.LimitsPolicy,
			Discard:   nats.DiscardOld,
			MaxAge:    30 * 24 * time.Hour,
			MaxMsgs:   2_000_000,
		})
	case err != nil:
		return fmt.Errorf("inspect stream %s: %w", StreamName, err)
	default:
		config := streamInfo.Config
		config.Subjects = wantedSubjects
		config.Retention = nats.LimitsPolicy
		config.Discard = nats.DiscardOld
		config.MaxAge = 30 * 24 * time.Hour
		config.MaxMsgs = 2_000_000
		_, err = js.UpdateStream(&config)
	}
	if err != nil {
		return fmt.Errorf("converge stream %s: %w", StreamName, err)
	}

	for _, kind := range []string{"audit", "usage"} {
		if err := ensureConsumer(js, bus, kind); err != nil {
			return err
		}
	}
	if bus.Plane == "control" {
		if err := ProvisionControlPlaneRuntime(ctx, js); err != nil {
			return err
		}
	}
	if bus.Plane == "model" {
		if err := ProvisionModelRuntime(ctx, js); err != nil {
			return err
		}
	}
	if bus.Plane == "application" {
		if err := ProvisionApplicationRuntime(ctx, js); err != nil {
			return err
		}
	}
	return nil
}

// ProvisionModelRuntime converges the fixed Model-plane streams and consumers
// used by runtime/session and scoped Application readers. Ordinary runtime
// principals can only bind or pull these exact resources; they cannot create,
// update, delete, or enumerate arbitrary topology.
func ProvisionModelRuntime(ctx context.Context, js nats.JetStreamContext) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	for _, stream := range []struct {
		name     string
		subjects []string
		maxAge   time.Duration
		maxBytes int64
	}{
		{name: ModelToolsStreamName, subjects: []string{"tools.completions.*"}, maxAge: 7 * 24 * time.Hour},
		{name: ModelOrchestrationStreamName, subjects: []string{"mp.v1.orchestration.>"}, maxAge: 24 * time.Hour},
		{name: ModelRunEventsStreamName, subjects: []string{"mp.v1.run.*.event"}, maxAge: 48 * time.Hour, maxBytes: 64 * 1024 * 1024},
	} {
		if err := ensureRuntimeStream(js, stream.name, stream.subjects, stream.maxAge, stream.maxBytes); err != nil {
			return err
		}
	}
	consumers := []struct {
		stream string
		config *nats.ConsumerConfig
	}{
		{stream: ModelToolsStreamName, config: &nats.ConsumerConfig{
			Durable: SessionToolsConsumerName, FilterSubject: "tools.completions.*",
			DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
			AckWait: 30 * time.Second, MaxDeliver: 5, ReplayPolicy: nats.ReplayInstantPolicy,
		}},
		{stream: ModelOrchestrationStreamName, config: &nats.ConsumerConfig{
			Durable: SessionOrchestrationConsumerName, FilterSubject: "mp.v1.orchestration.>",
			DeliverPolicy: nats.DeliverNewPolicy, AckPolicy: nats.AckNonePolicy,
			ReplayPolicy: nats.ReplayInstantPolicy,
		}},
		{stream: ModelRunEventsStreamName, config: &nats.ConsumerConfig{
			Durable: InsightRunConsumerName, DeliverSubject: InsightRunDeliverySubject,
			DeliverGroup: InsightRunConsumerName, FilterSubject: "mp.v1.run.*.event",
			DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
			AckWait: 30 * time.Second, MaxDeliver: 5, ReplayPolicy: nats.ReplayInstantPolicy,
		}},
		{stream: ModelOrchestrationStreamName, config: &nats.ConsumerConfig{
			Durable: InsightApprovalConsumerName, DeliverSubject: InsightApprovalDeliverySubject,
			DeliverGroup: InsightApprovalConsumerName, FilterSubject: "mp.v1.orchestration.approval",
			DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
			AckWait: 30 * time.Second, MaxDeliver: 5, ReplayPolicy: nats.ReplayInstantPolicy,
		}},
	}
	for _, consumer := range consumers {
		if err := ensureFixedConsumer(js, consumer.stream, consumer.config); err != nil {
			return err
		}
	}
	return nil
}

// ProvisionApplicationRuntime owns the Application lifecycle stream that
// notification-core publishes into. Runtime code only publishes with PubAck.
func ProvisionApplicationRuntime(ctx context.Context, js nats.JetStreamContext) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	for _, stream := range []struct {
		name     string
		subjects []string
	}{
		{name: ApplicationEventsStreamName, subjects: []string{"velion.application.>"}},
		{name: ApplicationModelStreamName, subjects: []string{"velion.model.>"}},
		{name: ApplicationIngestionStreamName, subjects: []string{"velion.ingestion.>"}},
	} {
		if err := ensureRuntimeStream(js, stream.name, stream.subjects, 14*24*time.Hour, 0); err != nil {
			return err
		}
	}
	consumers := []struct {
		stream string
		config *nats.ConsumerConfig
	}{
		{stream: ApplicationEventsStreamName, config: applicationPushConsumer(
			ConversationAIActionConsumerName, ConversationAIActionDelivery,
			"velion.application.conversation.ai_action.reviewed",
		)},
		{stream: ApplicationModelStreamName, config: applicationPushConsumer(
			ConversationModelActionConsumerName, ConversationModelActionDelivery,
			"velion.model.action.proposed",
		)},
		{stream: ApplicationIngestionStreamName, config: applicationPushConsumer(
			ConversationWebhookConsumerName, ConversationWebhookDelivery,
			"velion.ingestion.integration.webhook_received",
		)},
		{stream: ApplicationEventsStreamName, config: applicationPushConsumer(
			InsightMetricConsumerName, InsightMetricDelivery, "velion.application.>",
		)},
	}
	for _, consumer := range consumers {
		if err := ensureFixedConsumer(js, consumer.stream, consumer.config); err != nil {
			return err
		}
	}
	return nil
}

func applicationPushConsumer(durable, delivery, filter string) *nats.ConsumerConfig {
	return &nats.ConsumerConfig{
		Durable: durable, DeliverSubject: delivery, DeliverGroup: durable,
		FilterSubject: filter, DeliverPolicy: nats.DeliverAllPolicy,
		AckPolicy: nats.AckExplicitPolicy, AckWait: 30 * time.Second,
		MaxDeliver: 5, ReplayPolicy: nats.ReplayInstantPolicy,
	}
}

func ensureRuntimeStream(js nats.JetStreamContext, name string, subjects []string, maxAge time.Duration, maxBytes int64) error {
	info, err := js.StreamInfo(name)
	switch {
	case errors.Is(err, nats.ErrStreamNotFound):
		_, err = js.AddStream(&nats.StreamConfig{
			Name: name, Subjects: subjects, Storage: nats.FileStorage,
			Retention: nats.LimitsPolicy, Discard: nats.DiscardOld,
			MaxAge: maxAge, MaxMsgs: 1_000_000, MaxBytes: maxBytes,
		})
	case err != nil:
		return fmt.Errorf("inspect stream %s: %w", name, err)
	default:
		config := info.Config
		config.Subjects = append([]string(nil), subjects...)
		config.Retention = nats.LimitsPolicy
		config.Discard = nats.DiscardOld
		config.MaxAge = maxAge
		config.MaxMsgs = 1_000_000
		config.MaxBytes = maxBytes
		_, err = js.UpdateStream(&config)
	}
	if err != nil {
		return fmt.Errorf("converge stream %s: %w", name, err)
	}
	return nil
}

func ensureFixedConsumer(js nats.JetStreamContext, stream string, wanted *nats.ConsumerConfig) error {
	info, err := js.ConsumerInfo(stream, wanted.Durable)
	switch {
	case errors.Is(err, nats.ErrConsumerNotFound):
		_, err = js.AddConsumer(stream, wanted)
	case err != nil:
		return fmt.Errorf("inspect consumer %s: %w", wanted.Durable, err)
	default:
		got := info.Config
		if got.DeliverSubject != wanted.DeliverSubject || got.DeliverGroup != wanted.DeliverGroup ||
			got.FilterSubject != wanted.FilterSubject || got.DeliverPolicy != wanted.DeliverPolicy ||
			got.AckPolicy != wanted.AckPolicy || got.AckWait != wanted.AckWait ||
			got.ReplayPolicy != wanted.ReplayPolicy ||
			(wanted.MaxDeliver != 0 && got.MaxDeliver != wanted.MaxDeliver) {
			return fmt.Errorf("consumer %s exists with incompatible configuration; refusing destructive replacement", wanted.Durable)
		}
		return nil
	}
	if err != nil {
		return fmt.Errorf("create consumer %s: %w", wanted.Durable, err)
	}
	return nil
}

// ProvisionControlSharedRuntime owns the scoped Control-to-cross-plane stream
// and the fixed compatibility bridge consumer. Runtime producers and the
// bridge can only publish/bind/ACK; neither receives JetStream admin rights.
func ProvisionControlSharedRuntime(ctx context.Context, js nats.JetStreamContext) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	subjects := []string{
		"aqencia.controlplane.>",
		"notifications.>",
		"velion.session.>",
		"aqencia.reasoning.session.>",
		"velion.agent.>",
		"aqencia.reasoning.run.>",
		"app.session.>",
		ConvexControlDLQSubject,
		GDPRErasureRequestedSubject,
		GDPRErasureDLQSubject,
		GDPROwnershipTransferredSubject,
		DocumentsOrgPurgeDLQSubject,
		OrgDeletionSubjectWildcard,
	}
	if err := ensureControlStream(js, ControlSharedStreamName, subjects); err != nil {
		return err
	}
	wanted := &nats.ConsumerConfig{
		Durable: LegacyBridgeConsumerName, DeliverSubject: LegacyBridgeDelivery,
		DeliverGroup: LegacyBridgeConsumerName, FilterSubject: ">",
		DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
		AckWait: 30 * time.Second, MaxDeliver: 20, ReplayPolicy: nats.ReplayInstantPolicy,
	}
	info, err := js.ConsumerInfo(ControlSharedStreamName, LegacyBridgeConsumerName)
	switch {
	case errors.Is(err, nats.ErrConsumerNotFound):
		_, err = js.AddConsumer(ControlSharedStreamName, wanted)
	case err != nil:
		return fmt.Errorf("inspect consumer %s: %w", LegacyBridgeConsumerName, err)
	default:
		got := info.Config
		if got.DeliverSubject != wanted.DeliverSubject || got.DeliverGroup != wanted.DeliverGroup ||
			got.FilterSubject != wanted.FilterSubject || got.AckPolicy != wanted.AckPolicy ||
			got.MaxDeliver != wanted.MaxDeliver {
			return fmt.Errorf("consumer %s exists with incompatible configuration; refusing destructive replacement", LegacyBridgeConsumerName)
		}
	}
	if err != nil {
		return fmt.Errorf("create consumer %s: %w", LegacyBridgeConsumerName, err)
	}
	for _, consumer := range convexControlConsumerConfigs() {
		if err := ensureFixedConsumer(js, ControlSharedStreamName, consumer); err != nil {
			return err
		}
	}
	if err := ensureFixedConsumer(js, ControlSharedStreamName, gdprDocumentsConsumerConfig()); err != nil {
		return err
	}
	if err := ensureFixedConsumer(js, ControlSharedStreamName, documentsOrgErasureConsumerConfig()); err != nil {
		return err
	}
	if err := ensureFixedConsumer(js, ControlSharedStreamName, conversationOrgErasureConsumerConfig()); err != nil {
		return err
	}
	if err := ensureFixedConsumer(js, ControlSharedStreamName, sessionGDPRErasureConsumerConfig()); err != nil {
		return err
	}
	if err := ensureFixedConsumer(js, ControlSharedStreamName, quarryControlOrgErasureConsumerConfig()); err != nil {
		return err
	}
	for _, consumer := range notificationOrgDeletionConsumerConfigs() {
		if err := ensureFixedConsumer(js, ControlSharedStreamName, consumer); err != nil {
			return err
		}
	}
	if err := ensureFixedConsumer(js, ControlSharedStreamName, indexEngineOrgErasureConsumerConfig()); err != nil {
		return err
	}
	if err := ensureFixedConsumer(js, ControlSharedStreamName, graphIndexOrgErasureConsumerConfig()); err != nil {
		return err
	}
	if err := ensureFixedConsumer(js, ControlSharedStreamName, wikiStoreOrgErasureConsumerConfig()); err != nil {
		return err
	}
	if err := ensureFixedConsumer(js, ControlSharedStreamName, retrievalEngineOrgErasureConsumerConfig()); err != nil {
		return err
	}
	if err := ensureFixedConsumer(js, ControlSharedStreamName, dataQualityOrgErasureConsumerConfig()); err != nil {
		return err
	}
	if err := ensureFixedConsumer(js, ControlSharedStreamName, dataOrchestratorOrgErasureConsumerConfig()); err != nil {
		return err
	}
	if err := ensureFixedConsumer(js, ControlSharedStreamName, quickwitAdapterOrgErasureConsumerConfig()); err != nil {
		return err
	}
	if err := ensureFixedConsumer(js, ControlSharedStreamName, costCoreOrgErasureConsumerConfig()); err != nil {
		return err
	}
	if err := ensureFixedConsumer(js, ControlSharedStreamName, embeddingEngineOrgErasureConsumerConfig()); err != nil {
		return err
	}
	return nil
}

func gdprDocumentsConsumerConfig() *nats.ConsumerConfig {
	return &nats.ConsumerConfig{
		Durable: DocumentsGDPRConsumerName, DeliverSubject: DocumentsGDPRDeliverySubject,
		DeliverGroup: DocumentsGDPRConsumerName, FilterSubject: GDPRErasureRequestedSubject,
		DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
		AckWait: 30 * time.Second, MaxDeliver: 20, ReplayPolicy: nats.ReplayInstantPolicy,
	}
}

// documentsOrgErasureConsumerConfig is documents-api-go's second, org-scoped
// GDPR consumer (org_purge_consumer.go's orgPurgeDurableConsumerName). It
// mirrors gdprDocumentsConsumerConfig's shape exactly; both consumers run
// over the same shared connection/credentials in documents-api-go.
func documentsOrgErasureConsumerConfig() *nats.ConsumerConfig {
	return &nats.ConsumerConfig{
		Durable: DocumentsOrgErasureConsumerName, DeliverSubject: DocumentsOrgErasureDelivery,
		DeliverGroup: DocumentsOrgErasureConsumerName, FilterSubject: GDPRErasureRequestedSubject,
		DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
		AckWait: 30 * time.Second, MaxDeliver: 20, ReplayPolicy: nats.ReplayInstantPolicy,
	}
}

// sessionGDPRErasureConsumerConfig is Model Plane's session-core (Rust) pull
// consumer for the shared GDPR erasure fan-out. Unlike the push/queue
// consumers above, a pull consumer has no DeliverSubject/DeliverGroup: the
// client fetches with CONSUMER.MSG.NEXT instead of receiving an async
// delivery. ensureFixedConsumer's compatibility check tolerates the resulting
// empty DeliverSubject/DeliverGroup on both sides without modification.
func sessionGDPRErasureConsumerConfig() *nats.ConsumerConfig {
	return &nats.ConsumerConfig{
		Durable: SessionGDPRErasureConsumerName, FilterSubject: GDPRErasureRequestedSubject,
		DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
		AckWait: 30 * time.Second, MaxDeliver: 20, ReplayPolicy: nats.ReplayInstantPolicy,
	}
}

// quarryControlOrgErasureConsumerConfig is Ingestion Plane's quarry-control
// push/queue consumer for the shared GDPR erasure fan-out. The durable name
// must match subscriber.go's DurableConsumerName const, which switches from
// self-provisioning (nats.Durable) to nats.Bind once this entry exists.
func quarryControlOrgErasureConsumerConfig() *nats.ConsumerConfig {
	return &nats.ConsumerConfig{
		Durable: QuarryControlOrgErasureConsumerName, DeliverSubject: QuarryControlOrgErasureDelivery,
		DeliverGroup: QuarryControlOrgErasureConsumerName, FilterSubject: GDPRErasureRequestedSubject,
		DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
		AckWait: 30 * time.Second, MaxDeliver: 20, ReplayPolicy: nats.ReplayInstantPolicy,
	}
}

// notificationOrgDeletionConsumerConfigs returns notification-core's three
// fixed push/queue consumers, one per org-deletion lifecycle subject. Durable
// names must match org_deletion_consumer.go's orgDeletionDurablePrefix +
// "-pending"/"-reminder"/"-cancelled" suffixes exactly, since that consumer
// binds via nats.Bind once switched off self-provisioning.
func notificationOrgDeletionConsumerConfigs() []*nats.ConsumerConfig {
	definitions := []struct {
		durable  string
		delivery string
		filter   string
	}{
		{NotificationOrgDeletionPendingConsumerName, NotificationOrgDeletionPendingDelivery, OrgDeletionPendingSubject},
		{NotificationOrgDeletionReminderConsumerName, NotificationOrgDeletionReminderDelivery, OrgDeletionReminderSubject},
		{NotificationOrgDeletionCancelledConsumerName, NotificationOrgDeletionCancelledDelivery, OrgDeletionCancelledSubject},
	}
	configs := make([]*nats.ConsumerConfig, 0, len(definitions))
	for _, definition := range definitions {
		configs = append(configs, &nats.ConsumerConfig{
			Durable: definition.durable, DeliverSubject: definition.delivery,
			DeliverGroup: definition.durable, FilterSubject: definition.filter,
			DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
			AckWait: 30 * time.Second, MaxDeliver: 20, ReplayPolicy: nats.ReplayInstantPolicy,
		})
	}
	return configs
}

// indexEngineOrgErasureConsumerConfig is Data Plane v2's index-engine-rs GDPR
// org-erasure PULL consumer. Like sessionGDPRErasureConsumerConfig, a pull
// consumer has no DeliverSubject/DeliverGroup — index-engine-rs binds via
// get_consumer_from_stream (never get_stream first), so its identity also
// does not need $JS.API.STREAM.INFO.AQENCIA_CONTROLPLANE.
func indexEngineOrgErasureConsumerConfig() *nats.ConsumerConfig {
	return &nats.ConsumerConfig{
		Durable: IndexEngineOrgErasureConsumerName, FilterSubject: GDPRErasureRequestedSubject,
		DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
		AckWait: 30 * time.Second, MaxDeliver: 20, ReplayPolicy: nats.ReplayInstantPolicy,
	}
}

// graphIndexOrgErasureConsumerConfig is Data Plane v2's graph-index-rs GDPR
// org-erasure PULL consumer. Same shape and STREAM.INFO-free reasoning as
// indexEngineOrgErasureConsumerConfig above.
func graphIndexOrgErasureConsumerConfig() *nats.ConsumerConfig {
	return &nats.ConsumerConfig{
		Durable: GraphIndexOrgErasureConsumerName, FilterSubject: GDPRErasureRequestedSubject,
		DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
		AckWait: 30 * time.Second, MaxDeliver: 20, ReplayPolicy: nats.ReplayInstantPolicy,
	}
}

// wikiStoreOrgErasureConsumerConfig is Data Plane v2's wiki-store-go GDPR
// org-erasure push/queue consumer (nats.Bind + QueueSubscribe, ManualAck),
// mirroring documentsOrgErasureConsumerConfig's shape. The Durable value
// must exactly match wiki-store-go's internal/gdpr/subscriber.go
// DurableConsumerName, which also doubles as the queue-group name there.
func wikiStoreOrgErasureConsumerConfig() *nats.ConsumerConfig {
	return &nats.ConsumerConfig{
		Durable: WikiStoreOrgErasureConsumerName, DeliverSubject: WikiStoreOrgErasureDelivery,
		DeliverGroup: WikiStoreOrgErasureConsumerName, FilterSubject: GDPRErasureRequestedSubject,
		DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
		AckWait: 30 * time.Second, MaxDeliver: 20, ReplayPolicy: nats.ReplayInstantPolicy,
	}
}

// retrievalEngineOrgErasureConsumerConfig is Data Plane v2's
// retrieval-engine-rs GDPR org-erasure PULL consumer, matching
// sessionGDPRErasureConsumerConfig's shape (no DeliverSubject/DeliverGroup,
// no STREAM.INFO permission needed).
func retrievalEngineOrgErasureConsumerConfig() *nats.ConsumerConfig {
	return &nats.ConsumerConfig{
		Durable: RetrievalEngineOrgErasureConsumerName, FilterSubject: GDPRErasureRequestedSubject,
		DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
		AckWait: 30 * time.Second, MaxDeliver: 20, ReplayPolicy: nats.ReplayInstantPolicy,
	}
}

// dataQualityOrgErasureConsumerConfig is Data Plane v2's data-quality-go GDPR
// org-erasure push/queue consumer, mirroring documentsOrgErasureConsumerConfig's
// shape. The Durable value must exactly match data-quality-go's
// internal/gdpr/consumer.go OrgErasureDurableName.
func dataQualityOrgErasureConsumerConfig() *nats.ConsumerConfig {
	return &nats.ConsumerConfig{
		Durable: DataQualityOrgErasureConsumerName, DeliverSubject: DataQualityOrgErasureDelivery,
		DeliverGroup: DataQualityOrgErasureConsumerName, FilterSubject: GDPRErasureRequestedSubject,
		DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
		AckWait: 30 * time.Second, MaxDeliver: 20, ReplayPolicy: nats.ReplayInstantPolicy,
	}
}

// dataOrchestratorOrgErasureConsumerConfig is Data Plane v2's
// data-orchestrator-go GDPR org-erasure push/queue consumer, mirroring
// documentsOrgErasureConsumerConfig's shape. The Durable value must exactly
// match data-orchestrator-go's internal/gdpr/consumer.go durableConsumerName.
func dataOrchestratorOrgErasureConsumerConfig() *nats.ConsumerConfig {
	return &nats.ConsumerConfig{
		Durable: DataOrchestratorOrgErasureConsumerName, DeliverSubject: DataOrchestratorOrgErasureDelivery,
		DeliverGroup: DataOrchestratorOrgErasureConsumerName, FilterSubject: GDPRErasureRequestedSubject,
		DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
		AckWait: 30 * time.Second, MaxDeliver: 20, ReplayPolicy: nats.ReplayInstantPolicy,
	}
}

// quickwitAdapterOrgErasureConsumerConfig is Data Plane v2's
// quickwit-adapter-rs GDPR org-erasure PULL consumer. Unlike the other pull
// consumers above, quickwit-adapter-rs's client resolves the consumer via
// js.get_stream(..) before get_consumer(..), so this identity also needs
// $JS.API.STREAM.INFO.AQENCIA_CONTROLPLANE granted in control-shared-nats.conf
// (see the session-core-gdpr identity for the same requirement).
func quickwitAdapterOrgErasureConsumerConfig() *nats.ConsumerConfig {
	return &nats.ConsumerConfig{
		Durable: QuickwitAdapterOrgErasureConsumerName, FilterSubject: GDPRErasureRequestedSubject,
		DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
		AckWait: 30 * time.Second, MaxDeliver: 20, ReplayPolicy: nats.ReplayInstantPolicy,
	}
}

// costCoreOrgErasureConsumerConfig is Model Plane's cost-core GDPR
// org-erasure push/queue consumer (nats.Bind + QueueSubscribe via
// DurableConsumer.BindProvisioned), mirroring documentsOrgErasureConsumerConfig's
// shape. The Durable value must exactly match cost-core's
// internal/consumers/org_erasure_consumer.go orgErasureDurable.
func costCoreOrgErasureConsumerConfig() *nats.ConsumerConfig {
	return &nats.ConsumerConfig{
		Durable: CostCoreOrgErasureConsumerName, DeliverSubject: CostCoreOrgErasureDelivery,
		DeliverGroup: CostCoreOrgErasureConsumerName, FilterSubject: GDPRErasureRequestedSubject,
		DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
		AckWait: 30 * time.Second, MaxDeliver: 20, ReplayPolicy: nats.ReplayInstantPolicy,
	}
}

// embeddingEngineOrgErasureConsumerConfig is Data Plane v2's
// embedding-engine-rs GDPR org-erasure PULL consumer, purging org-scoped
// Qdrant vector points (dataplane_knowledge / wiki_block_embeddings / the
// visual page-image collection). Same shape and STREAM.INFO-free reasoning
// as indexEngineOrgErasureConsumerConfig/graphIndexOrgErasureConsumerConfig
// above — embedding-engine-rs binds via get_consumer_from_stream (never
// get_stream first), so its identity also does not need
// $JS.API.STREAM.INFO.AQENCIA_CONTROLPLANE.
func embeddingEngineOrgErasureConsumerConfig() *nats.ConsumerConfig {
	return &nats.ConsumerConfig{
		Durable: EmbeddingEngineOrgErasureConsumerName, FilterSubject: GDPRErasureRequestedSubject,
		DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
		AckWait: 30 * time.Second, MaxDeliver: 20, ReplayPolicy: nats.ReplayInstantPolicy,
	}
}

// conversationOrgErasureConsumerConfig is conversation-core-go's fixed
// consumer for the same shared GDPR erasure fan-out (see
// gdprDocumentsConsumerConfig for documents-api-go's sibling registration).
// The Durable value must exactly match orgErasureDurable in
// conversation-core-go/internal/consumers/org_erasure_consumer.go — that
// consumer binds via nats.Bind, which requires this durable to already exist
// server-side, so a name mismatch here silently breaks the org-scoped
// hard-purge.
func conversationOrgErasureConsumerConfig() *nats.ConsumerConfig {
	return &nats.ConsumerConfig{
		Durable: ConversationOrgErasureConsumerName, DeliverSubject: ConversationOrgErasureDelivery,
		DeliverGroup: ConversationOrgErasureConsumerName, FilterSubject: GDPRErasureRequestedSubject,
		DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
		AckWait: 30 * time.Second, MaxDeliver: 20, ReplayPolicy: nats.ReplayInstantPolicy,
	}
}

func convexControlConsumerConfigs() []*nats.ConsumerConfig {
	definitions := []struct {
		durable  string
		delivery string
		filter   string
	}{
		{"convex-org-created-v1", "_VELION.CONTROL.SHARED.DELIVER.application.convex.org-created", "aqencia.controlplane.org.created"},
		{"convex-org-updated-v1", "_VELION.CONTROL.SHARED.DELIVER.application.convex.org-updated", "aqencia.controlplane.org.updated"},
		{"convex-org-deleted-v1", "_VELION.CONTROL.SHARED.DELIVER.application.convex.org-deleted", "aqencia.controlplane.org.deleted"},
		{"convex-org-member-added-v1", "_VELION.CONTROL.SHARED.DELIVER.application.convex.member-added", "aqencia.controlplane.org.member_added"},
		{"convex-org-member-removed-v1", "_VELION.CONTROL.SHARED.DELIVER.application.convex.member-removed", "aqencia.controlplane.org.member_removed"},
		{"convex-org-changed-v2", "_VELION.CONTROL.SHARED.DELIVER.application.convex.org-changed", "aqencia.controlplane.org.changed"},
		{"convex-org-member-changed-v2", "_VELION.CONTROL.SHARED.DELIVER.application.convex.member-changed", "aqencia.controlplane.org.member_changed"},
	}
	configs := make([]*nats.ConsumerConfig, 0, len(definitions))
	for _, definition := range definitions {
		configs = append(configs, &nats.ConsumerConfig{
			Durable: definition.durable, DeliverSubject: definition.delivery,
			DeliverGroup: definition.durable, FilterSubject: definition.filter,
			DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
			AckWait: 30 * time.Second, MaxDeliver: 100,
			ReplayPolicy: nats.ReplayInstantPolicy,
		})
	}
	return configs
}

// ProvisionControlPlaneRuntime converges domain streams and fixed durable
// consumers with deployment-only credentials. Runtime services never receive
// stream/consumer create, update, delete, purge, or list authority.
func ProvisionControlPlaneRuntime(ctx context.Context, js nats.JetStreamContext) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	streams := []struct {
		name     string
		subjects []string
	}{
		{name: AuthEventsStreamName, subjects: []string{"auth.>"}},
		{name: ControlEventsStreamName, subjects: []string{"user.>", "organization.>", "session.created", "session.ended", "billing.>", "usage.>"}},
	}
	for _, stream := range streams {
		if err := ensureControlStream(js, stream.name, stream.subjects); err != nil {
			return err
		}
	}
	return ensureBillingPlanConsumer(js)
}

func ensureControlStream(js nats.JetStreamContext, name string, subjects []string) error {
	info, err := js.StreamInfo(name)
	switch {
	case errors.Is(err, nats.ErrStreamNotFound):
		_, err = js.AddStream(&nats.StreamConfig{
			Name: name, Subjects: subjects, Storage: nats.FileStorage,
			Retention: nats.LimitsPolicy, Discard: nats.DiscardOld,
			MaxAge: 7 * 24 * time.Hour, MaxMsgs: 1_000_000,
		})
	case err != nil:
		return fmt.Errorf("inspect stream %s: %w", name, err)
	default:
		config := info.Config
		config.Subjects = append([]string(nil), subjects...)
		config.Retention = nats.LimitsPolicy
		config.Discard = nats.DiscardOld
		config.MaxAge = 7 * 24 * time.Hour
		config.MaxMsgs = 1_000_000
		_, err = js.UpdateStream(&config)
	}
	if err != nil {
		return fmt.Errorf("converge stream %s: %w", name, err)
	}
	return nil
}

func ensureBillingPlanConsumer(js nats.JetStreamContext) error {
	wanted := &nats.ConsumerConfig{
		Durable: BillingPlanConsumerName, DeliverSubject: BillingPlanDeliverySubject,
		DeliverGroup: BillingPlanConsumerName, FilterSubject: BillingPlanSubject,
		DeliverPolicy: nats.DeliverAllPolicy, AckPolicy: nats.AckExplicitPolicy,
		AckWait: 30 * time.Second, MaxDeliver: 5, ReplayPolicy: nats.ReplayInstantPolicy,
	}
	info, err := js.ConsumerInfo(ControlEventsStreamName, BillingPlanConsumerName)
	switch {
	case errors.Is(err, nats.ErrConsumerNotFound):
		_, err = js.AddConsumer(ControlEventsStreamName, wanted)
	case err != nil:
		return fmt.Errorf("inspect consumer %s: %w", BillingPlanConsumerName, err)
	default:
		got := info.Config
		if got.Durable != wanted.Durable || got.DeliverSubject != wanted.DeliverSubject ||
			got.DeliverGroup != wanted.DeliverGroup || got.FilterSubject != wanted.FilterSubject ||
			got.DeliverPolicy != wanted.DeliverPolicy || got.AckPolicy != wanted.AckPolicy ||
			got.MaxDeliver != wanted.MaxDeliver {
			return fmt.Errorf("consumer %s exists with incompatible configuration; refusing destructive replacement", BillingPlanConsumerName)
		}
		return nil
	}
	if err != nil {
		return fmt.Errorf("create consumer %s: %w", BillingPlanConsumerName, err)
	}
	return nil
}

func ensureConsumer(js nats.JetStreamContext, bus Bus, kind string) error {
	name := ConsumerName(bus.Name, kind)
	wanted := &nats.ConsumerConfig{
		Durable:        name,
		DeliverSubject: DeliverySubject(bus.Plane, kind),
		DeliverGroup:   name,
		FilterSubject:  PlaneSubject(kind, bus.Plane),
		DeliverPolicy:  nats.DeliverAllPolicy,
		AckPolicy:      nats.AckExplicitPolicy,
		AckWait:        30 * time.Second,
		MaxDeliver:     5,
		ReplayPolicy:   nats.ReplayInstantPolicy,
	}
	info, err := js.ConsumerInfo(StreamName, name)
	switch {
	case errors.Is(err, nats.ErrConsumerNotFound):
		_, err = js.AddConsumer(StreamName, wanted)
	case err != nil:
		return fmt.Errorf("inspect consumer %s: %w", name, err)
	default:
		got := info.Config
		if got.Durable != wanted.Durable || got.DeliverSubject != wanted.DeliverSubject || got.DeliverGroup != wanted.DeliverGroup || got.FilterSubject != wanted.FilterSubject || got.AckPolicy != wanted.AckPolicy || got.DeliverPolicy != wanted.DeliverPolicy || got.MaxDeliver != wanted.MaxDeliver {
			return fmt.Errorf("consumer %s exists with incompatible configuration; refusing destructive replacement", name)
		}
		return nil
	}
	if err != nil {
		return fmt.Errorf("create consumer %s: %w", name, err)
	}
	return nil
}
