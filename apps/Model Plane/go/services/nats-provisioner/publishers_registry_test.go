package main

// The registry of everything in the Model Plane that publishes to NATS.
//
// This is the input of manifest_contract_test.go: every row names who publishes,
// as which broker principal, on which subject, with what delivery guarantee, and
// where in the source that is true. The contract tests tie the rows to the real
// code (a row whose source text is gone fails), to the real broker config
// (deploy/nats.conf: may that principal publish that subject?) and to the real
// manifest (does a stream capture it?). They also fail when the code gains a
// publisher this file does not list, so adding one forces the decision "which
// stream carries it" to be made here, in review, instead of in production.

type transport string

const (
	// jetStreamAck: the publisher waits for the stream's PubAck. With no stream
	// capturing the subject the publish fails ("no stream found for given
	// subject") and, behind an outbox, burns its attempts. A stream is REQUIRED.
	jetStreamAck transport = "jetstream-ack"

	// coreNATS: fire and forget. A missing stream is invisible to the publisher
	// (there is no ack to fail), and a missing publish grant is an asynchronous
	// error the publisher never sees. Delivery is at most once, to whoever is
	// subscribed at that instant, unless a stream happens to capture the subject.
	coreNATS transport = "core-nats"
)

type publisherRow struct {
	// service and natsUser: who publishes, and the broker principal it connects
	// as (deploy/nats.conf); inbox is the custom inbox prefix its client sets.
	service  string
	natsUser string
	inbox    string

	// subject is a concrete subject the service publishes (tenant tokens are x).
	subject   string
	transport transport

	// builder and args, when set, name the mp-events subject builder that makes
	// subject; the contract test evaluates the real builder's format string.
	builder string
	args    []string

	// stream is the manifest stream that must capture subject; "" means no
	// stream is required or provided (core NATS only).
	stream string

	// source is the file, relative to apps/Model Plane, that publishes it, and
	// needle text that must appear in it.
	source string
	needle string

	// gap is a defect that is KNOWN and NOT fixed by the change that introduced
	// this registry. The ACL test requires the gap to still be real (so fixing
	// deploy/nats.conf forces this row to be updated) and reports it.
	gap string
}

const (
	sessionCoreUser = "session-core-runtime"
	sessionCoreBox  = "_INBOX.SESSION_CORE_RUNTIME"
	gatewayUser     = "model-gateway-runtime"
	gatewayBox      = "_INBOX.MODEL_GATEWAY_RUNTIME"
	orchestratorUsr = "orchestrator-core-runtime"
	orchestratorBox = "_INBOX.ORCHESTRATOR_CORE_RUNTIME"
	capabilityUser  = "capability-core-runtime"
	capabilityBox   = "_INBOX.CAPABILITY_CORE_RUNTIME"

	gatewayGrant = "deploy/nats.conf grants model-gateway-runtime no publish on this subject: the broker drops it, " +
		"and the publisher (core NATS, fire-and-forget) never learns"
	orchestratorGrant = "deploy/nats.conf grants orchestrator-core-runtime only mp.v1.run.*.event: " +
		"orchestration events are dropped by the broker, and the publisher (core NATS) never learns"
)

func publisherRegistry() []publisherRow {
	return []publisherRow{
		// ---- session-core: durable outbox publishers. ----
		// Every row below leaves through session_audit_outbox, which marks a row
		// published only after the stream's PubAck.
		{
			service: "session-core", natsUser: sessionCoreUser, inbox: sessionCoreBox,
			subject: "verevon.audit.v2.model.session-core.tool_action", transport: jetStreamAck,
			stream: auditObservabilityStreamName,
			source: "rust/services/session-core/src/audit_publisher.rs",
			needle: `SUBJECT_MODEL_TOOL_ACTION: &str = "verevon.audit.v2.model.session-core.tool_action"`,
		},
		{
			service: "session-core", natsUser: sessionCoreUser, inbox: sessionCoreBox,
			subject: "verevon.audit.v2.model.session-core.tool_action_reserved", transport: jetStreamAck,
			stream: auditObservabilityStreamName,
			source: "rust/services/session-core/src/audit_publisher.rs",
			needle: `EVENT_TOOL_ACTION_RESERVED: &str = "tool_action_reserved"`,
		},
		{
			service: "session-core", natsUser: sessionCoreUser, inbox: sessionCoreBox,
			subject: "mp.v1.run.x.event", transport: jetStreamAck,
			stream: "MODEL_PLANE_RUN_EVENTS",
			source: "rust/services/session-core/src/audit_publisher.rs",
			needle: `RUN_EVENT_SUBJECT_PREFIX: &str = "mp.v1.run."`,
		},

		// ---- model-gateway: core NATS through mp-events' EventPublisher. ----
		{
			service: "model-gateway", natsUser: gatewayUser, inbox: gatewayBox,
			subject: "mp.v1.run.x.event", transport: coreNATS, builder: "run_event_subject", args: []string{"x"},
			stream: "MODEL_PLANE_RUN_EVENTS",
			source: "rust/services/model-gateway/src/sse.rs", needle: "subjects::run_event_subject(&run.run_id)",
		},
		{
			service: "model-gateway", natsUser: gatewayUser, inbox: gatewayBox,
			subject: "mp.v1.usage.x", transport: jetStreamAck, builder: "usage_subject", args: []string{"x"},
			stream: "MODEL_PLANE_USAGE_EVENTS",
			source: "rust/services/model-gateway/src/nats_publisher.rs", needle: `subject.starts_with("mp.v1.usage.")`,
		},
		{
			service: "model-gateway", natsUser: gatewayUser, inbox: gatewayBox,
			subject: "mp.v1.stream.opened", transport: coreNATS, builder: "stream_subject", args: []string{"opened"},
			source: "rust/services/model-gateway/src/sse.rs", needle: `subjects::stream_subject("opened")`,
		},
		{
			service: "model-gateway", natsUser: gatewayUser, inbox: gatewayBox,
			subject: "mp.v1.stream.closed", transport: coreNATS, builder: "stream_subject", args: []string{"closed"},
			source: "rust/services/model-gateway/src/sse.rs", needle: `subjects::stream_subject("closed")`,
		},
		{
			service: "model-gateway", natsUser: gatewayUser, inbox: gatewayBox,
			subject: "mp.v1.ingress.accepted", transport: coreNATS, builder: "ingress_subject", args: []string{"accepted"},
			source: "rust/services/model-gateway/src/grpc.rs", needle: `subjects::ingress_subject("accepted")`,
		},
		{
			service: "model-gateway", natsUser: gatewayUser, inbox: gatewayBox,
			subject: "mp.v1.orchestration.run", transport: coreNATS,
			stream: "MP_ORCHESTRATION_EVENTS",
			source: "rust/services/model-gateway/src/http_routes.rs", needle: "mp_events::subjects::SUBJECT_RUN",
		},
		{
			service: "model-gateway", natsUser: gatewayUser, inbox: gatewayBox,
			subject: "mp.v1.feedback.rated", transport: coreNATS,
			source: "rust/services/model-gateway/src/implicit_feedback.rs",
			needle: `FEEDBACK_SUBJECT: &str = "mp.v1.feedback.rated"`,
		},
		{
			service: "model-gateway", natsUser: gatewayUser, inbox: gatewayBox,
			subject: "mp.v1.finetune.x.created", transport: coreNATS, builder: "finetune_event_subject", args: []string{"x", "created"},
			source: "rust/services/model-gateway/src/finetune_routes.rs", needle: "subjects::finetune_event_subject(org_id, event_type)",
		},
		{
			service: "model-gateway", natsUser: gatewayUser, inbox: gatewayBox,
			subject: "mp.v1.security.x", transport: coreNATS, builder: "security_subject", args: []string{"x"},
			source: "rust/services/model-gateway/src/tool_loop.rs", needle: "subjects::security_subject(org_id)",
			gap: gatewayGrant + " (tool-result screening events)",
		},
		{
			service: "model-gateway", natsUser: gatewayUser, inbox: gatewayBox,
			subject: "agents.approval.requested", transport: coreNATS,
			source: "rust/services/model-gateway/src/approvals.rs", needle: `"agents.approval.requested"`,
			gap: gatewayGrant + ", and no stream or in-repo consumer exists for it",
		},
		{
			service: "model-gateway", natsUser: gatewayUser, inbox: gatewayBox,
			subject: "agents.approval.resolved", transport: coreNATS,
			source: "rust/services/model-gateway/src/approvals.rs", needle: `"agents.approval.resolved"`,
			gap: gatewayGrant + ", and no stream or in-repo consumer exists for it",
		},

		// ---- orchestrator-core: core NATS through pkg/natsx. ----
		{
			service: "orchestrator-core", natsUser: orchestratorUsr, inbox: orchestratorBox,
			subject: "mp.v1.run.x.event", transport: coreNATS,
			stream: "MODEL_PLANE_RUN_EVENTS",
			source: "go/services/orchestrator-core/cmd/activities/activities.go", needle: "natsx.RunEventSubject(runID)",
		},
		{
			service: "orchestrator-core", natsUser: orchestratorUsr, inbox: orchestratorBox,
			subject: "mp.v1.orchestration.plan", transport: coreNATS, stream: "MP_ORCHESTRATION_EVENTS",
			source: "go/services/orchestrator-core/internal/orchestration/types.go", needle: `"mp.v1.orchestration.plan"`,
			gap: orchestratorGrant,
		},
		{
			service: "orchestrator-core", natsUser: orchestratorUsr, inbox: orchestratorBox,
			subject: "mp.v1.orchestration.todo", transport: coreNATS, stream: "MP_ORCHESTRATION_EVENTS",
			source: "go/services/orchestrator-core/internal/orchestration/types.go", needle: `"mp.v1.orchestration.todo"`,
			gap: orchestratorGrant,
		},
		{
			service: "orchestrator-core", natsUser: orchestratorUsr, inbox: orchestratorBox,
			subject: "mp.v1.orchestration.approval", transport: coreNATS, stream: "MP_ORCHESTRATION_EVENTS",
			source: "go/services/orchestrator-core/internal/orchestration/types.go", needle: `"mp.v1.orchestration.approval"`,
			gap: orchestratorGrant + " (insight-core's approval durable reads this stream)",
		},
		{
			service: "orchestrator-core", natsUser: orchestratorUsr, inbox: orchestratorBox,
			subject: "mp.v1.orchestration.subagent", transport: coreNATS, stream: "MP_ORCHESTRATION_EVENTS",
			source: "go/services/orchestrator-core/internal/orchestration/types.go", needle: `"mp.v1.orchestration.subagent"`,
			gap: orchestratorGrant,
		},
		{
			service: "orchestrator-core", natsUser: orchestratorUsr, inbox: orchestratorBox,
			subject: "mp.v1.orchestration.run", transport: coreNATS, stream: "MP_ORCHESTRATION_EVENTS",
			source: "go/services/orchestrator-core/internal/orchestration/types.go", needle: `"mp.v1.orchestration.run"`,
			gap: orchestratorGrant,
		},

		// ---- capability-core: core NATS through pkg/publisher -> pkg/natsx. ----
		{
			service: "capability-core", natsUser: capabilityUser, inbox: capabilityBox,
			subject: "mp.v1.capability.mcp_server.registered", transport: coreNATS,
			source: "go/services/capability-core/internal/reconcile/reconcile.go",
			needle: `const SubjectPrefix = "mp.v1.capability"`,
		},
		{
			service: "capability-core", natsUser: capabilityUser, inbox: capabilityBox,
			subject: "mp.v1.capability.task.dispatched", transport: coreNATS,
			source: "go/services/capability-core/internal/taskexec/executor.go",
			needle: "mp.v1.capability.task.dispatched",
		},
	}
}

// unusedSubjectBuilders are the mp-events subject builders that no Model Plane
// code calls today. The contract test fails if one gains a caller (it must then
// be registered above) and if a registered builder is listed here.
func unusedSubjectBuilders() map[string]string {
	return map[string]string{
		"event_subject":                    "generic helper, no caller",
		"session_command_subject":          "mp.v1.session.<key>.command has no publisher in the Model Plane",
		"retrieval_fetched_subject":        "documented as emitted by session-core, but nothing publishes it",
		"retrieval_used_subject":           "documented as emitted by session-core, but nothing publishes it",
		"retrieval_low_confidence_subject": "documented as emitted by session-core, but nothing publishes it",
		"legacy_run_event_subject":         "compat-window mirror, reached only through translate_new_to_legacy",
		"legacy_session_command_subject":   "compat-window mirror, reached only through translate_new_to_legacy",
	}
}

// natsClientModules lists every module in the Model Plane that links a NATS
// client, with its role. A module that gains one must be added here; the test
// then forces a look at what it publishes.
func natsClientModules() map[string]string {
	return map[string]string{
		"go/services/capability-core":   "publisher (core NATS, registry above) and JetStream consumer",
		"go/services/cost-core":         "consumer only (mp.v1.usage.* core subscription, shared-broker GDPR consumer)",
		"go/services/nats-provisioner":  "provisioner",
		"go/services/orchestrator-core": "publisher (core NATS, registry above) and subscriber",
		"rust/services/model-gateway":   "publisher (core NATS, registry above) and subscriber",
		"rust/services/session-core":    "JetStream publisher (registry above) and JetStream consumers",
		"rust/crates/mp-events":         "subject catalogue and publisher trait; links the client for its JetStream specs only",
	}
}
