package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"testing"
	"time"

	natsserver "github.com/nats-io/nats-server/v2/server"
	"github.com/nats-io/nats.go"

	"github.com/triodelab/controlplane/audit-core/internal/events"
	"github.com/triodelab/controlplane/audit-core/internal/provisioner"
	"github.com/triodelab/controlplane/audit-core/internal/store"
	"github.com/triodelab/controlplane/audit-core/internal/subscriber"
)

const serverTestCredentials = `[{"principal":"velion-gateway","audience":"audit-core","token":"0123456789abcdef0123456789abcdef","scopes":["audit:read:self"]},{"principal":"integration-corev2","audience":"audit-core","token":"abcdef0123456789abcdef0123456789","scopes":["audit:write"],"planes":["ingestion"]}]`

func TestLoadConfigRejectsIncompleteServiceCredentialAuthority(t *testing.T) {
	setMinimumConfig(t)
	t.Setenv("AUDIT_CORE_SERVICE_CREDENTIALS", `[{"principal":"velion-gateway","audience":"audit-core","token":"0123456789abcdef0123456789abcdef","scopes":["audit:read:self"]}]`)
	if _, err := loadConfig(); err == nil {
		t.Fatal("incomplete service credential authority was accepted at startup")
	}
}

func TestDeploymentWiresNamedCredentialedExtraPlaneBuses(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test path")
	}
	controlRoot := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
	controlCompose, err := os.ReadFile(filepath.Join(controlRoot, "docker-compose.yml"))
	if err != nil {
		t.Fatal(err)
	}
	controlText := string(controlCompose)
	if !strings.Contains(controlText, "AUDIT_EXTRA_NATS_BUSES:") || strings.Contains(controlText, "EXTRA_NATS_URLS:") {
		t.Fatal("Control compose must use named credentialed Audit bus configuration only")
	}
	for _, required := range []string{
		"AUDIT_CONTROL_NATS_PASSWORD",
		"AUDIT_MODEL_NATS_PASSWORD",
		"AUDIT_APPLICATION_NATS_PASSWORD",
		"audit-nats-provisioner:",
		"condition: service_completed_successfully",
	} {
		if !strings.Contains(controlText, required) {
			t.Fatalf("Control compose scoped NATS deployment contract missing %q", required)
		}
	}
	if strings.Contains(controlText, `"token":"${MODEL_NATS_TOKEN`) || strings.Contains(controlText, `"token":"${APPLICATION_NATS_TOKEN`) {
		t.Fatal("audit-core still receives a plane-wide broker token")
	}

	applicationCompose, err := os.ReadFile(filepath.Join(controlRoot, "..", "Application Plane", "docker-compose.yml"))
	if err != nil {
		t.Fatal(err)
	}
	applicationText := string(applicationCompose)
	start := strings.Index(applicationText, "  nats:\n")
	end := strings.Index(applicationText[start:], "  application-prometheus:\n")
	if start < 0 || end < 0 {
		t.Fatal("Application NATS service block not found")
	}
	natsBlock := applicationText[start : start+end]
	if !strings.Contains(natsBlock, "inter-plane-bus:\n        aliases:\n          - application-nats") {
		t.Fatal("Application NATS is not reachable by stable alias on inter-plane-bus")
	}
	leadsStart := strings.Index(applicationText, "  leads-core:\n")
	leadsEnd := strings.Index(applicationText[leadsStart:], "  information-core:\n")
	if leadsStart < 0 || leadsEnd < 0 {
		t.Fatal("Application leads-core block not found")
	}
	leadsBlock := applicationText[leadsStart : leadsStart+leadsEnd]
	if !strings.Contains(leadsBlock, "NATS_URL: nats://application-nats:4222") ||
		!strings.Contains(leadsBlock, "NATS_USER: application-leads") ||
		!strings.Contains(leadsBlock, "NATS_PASSWORD: ${APPLICATION_LEADS_NATS_PASSWORD:") {
		t.Fatal("Application audit producer is not pinned to the Application plane broker")
	}
	for _, path := range []string{
		filepath.Join(controlRoot, "nats.conf"),
		filepath.Join(controlRoot, "..", "Model Plane", "deploy", "nats.conf"),
		filepath.Join(controlRoot, "..", "Application Plane", "nats.conf"),
	} {
		contents, readErr := os.ReadFile(path)
		if readErr != nil {
			t.Fatalf("read scoped broker config %s: %v", path, readErr)
		}
		text := string(contents)
		if !strings.Contains(text, "users") || !strings.Contains(text, "audit-core-") || strings.Contains(text, "token: $NATS_TOKEN") {
			t.Fatalf("broker config %s is not multi-user scoped", path)
		}
	}
}

func TestRequiredProvisioningIsIndependentFromOptionalExtraPlanes(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test path")
	}
	controlRoot := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
	composeBytes, err := os.ReadFile(filepath.Join(controlRoot, "docker-compose.yml"))
	if err != nil {
		t.Fatal(err)
	}
	compose := string(composeBytes)
	required, err := composeServiceBlock(compose, "audit-nats-provisioner")
	if err != nil {
		t.Fatal(err)
	}
	for _, forbidden := range []string{"model-nats", "application-nats", "MODEL_NATS_PROVISIONER_PASSWORD", "APPLICATION_NATS_PROVISIONER_PASSWORD", "inter-plane-bus"} {
		if strings.Contains(required, forbidden) {
			t.Fatalf("required local provisioner still depends on optional extra plane %q", forbidden)
		}
	}
	for _, requiredText := range []string{"controlplane-nats", "control-shared-nats", `"name":"control"`, `"name":"shared-control"`} {
		if !strings.Contains(required, requiredText) {
			t.Fatalf("required local provisioner is missing %q", requiredText)
		}
	}

	extra, err := composeServiceBlock(compose, "audit-extra-nats-provisioner")
	if err != nil {
		t.Fatal(err)
	}
	for _, requiredText := range []string{"restart: on-failure", "model-nats", "application-nats", "MODEL_NATS_PROVISIONER_PASSWORD", "APPLICATION_NATS_PROVISIONER_PASSWORD", "inter-plane-bus"} {
		if !strings.Contains(extra, requiredText) {
			t.Fatalf("optional extra-plane provisioner is missing %q", requiredText)
		}
	}
	for _, service := range []string{"auth-core", "user-core", "org-core", "billing-core", "session-core", "audit-core"} {
		block, blockErr := composeServiceBlock(compose, service)
		if blockErr != nil {
			t.Fatal(blockErr)
		}
		if strings.Contains(block, "audit-extra-nats-provisioner") {
			t.Fatalf("%s startup is blocked by optional extra-plane provisioning", service)
		}
	}
}

func TestAuditHealthcheckUsesControlHealthNotOptionalPlaneReadiness(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test path")
	}
	controlRoot := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
	composeBytes, err := os.ReadFile(filepath.Join(controlRoot, "docker-compose.yml"))
	if err != nil {
		t.Fatal(err)
	}
	audit, err := composeServiceBlock(string(composeBytes), "audit-core")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(audit, "http://127.0.0.1:8187/healthz") {
		t.Fatal("Audit Docker healthcheck must use local Control-plane health")
	}
	if strings.Contains(audit, "http://127.0.0.1:8187/readyz") {
		t.Fatal("Audit Docker healthcheck must not gate startup on optional plane readiness")
	}
}

func TestPlaneRuntimePrincipalsCannotReachJetStreamAdminOrAuditConsumerSubjects(t *testing.T) {
	const password = "0123456789abcdef0123456789abcdef"
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test path")
	}
	controlRoot := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
	planes := []struct {
		name           string
		path           string
		runtimeUser    string
		inbox          string
		allowedSubject string
		roundTrip      bool
		delivery       string
		ack            string
	}{
		{name: "model", path: filepath.Join(controlRoot, "..", "Model Plane", "deploy", "nats.conf"), runtimeUser: "model-runtime", inbox: "_INBOX.MODEL_RUNTIME", allowedSubject: "mp.v1.run.fixture.event", roundTrip: true, delivery: "_VELION.AUDIT.DELIVER.model.audit-v2", ack: "$JS.ACK.VELION_CONTROL_OBSERVABILITY.audit-core-model-v3-audit.1.1.1.1.1"},
		{name: "application", path: filepath.Join(controlRoot, "..", "Application Plane", "nats.conf"), runtimeUser: "application-social", inbox: "_INBOX.APPLICATION_SOCIAL", allowedSubject: "velion.application.social.fixture", delivery: "_VELION.AUDIT.DELIVER.application.audit-v2", ack: "$JS.ACK.VELION_CONTROL_OBSERVABILITY.audit-core-application-v3-audit.1.1.1.1.1"},
	}
	for _, envName := range []string{
		"MODEL_NATS_RUNTIME_PASSWORD", "MODEL_GATEWAY_NATS_PASSWORD", "MODEL_SESSION_CORE_NATS_PASSWORD",
		"AUDIT_MODEL_NATS_PASSWORD", "MODEL_NATS_PROVISIONER_PASSWORD",
		"APPLICATION_CONVEX_MODEL_NATS_PASSWORD", "APPLICATION_INSIGHT_MODEL_NATS_PASSWORD",
		"APPLICATION_CONVERSATION_NATS_PASSWORD", "APPLICATION_SOCIAL_NATS_PASSWORD", "APPLICATION_INSIGHT_NATS_PASSWORD",
		"APPLICATION_LEADS_NATS_PASSWORD", "APPLICATION_NOTIFICATION_NATS_PASSWORD",
		"AUDIT_APPLICATION_NATS_PASSWORD", "APPLICATION_NATS_PROVISIONER_PASSWORD",
	} {
		t.Setenv(envName, strconv.Quote(password))
	}
	for _, plane := range planes {
		t.Run(plane.name, func(t *testing.T) {
			options, err := natsserver.ProcessConfigFile(plane.path)
			if err != nil {
				t.Fatal(err)
			}
			options.Port = -1
			options.HTTPPort = -1
			options.StoreDir = t.TempDir()
			instance, err := natsserver.NewServer(options)
			if err != nil {
				t.Fatal(err)
			}
			go instance.Start()
			if !instance.ReadyForConnections(10 * time.Second) {
				t.Fatal("scoped broker did not start")
			}
			t.Cleanup(func() { instance.Shutdown(); instance.WaitForShutdown() })

			permissionErrors := make(chan error, 8)
			runtimeConnection, err := nats.Connect(instance.ClientURL(),
				nats.UserInfo(plane.runtimeUser, password),
				nats.CustomInboxPrefix(plane.inbox),
				nats.ErrorHandler(func(_ *nats.Conn, _ *nats.Subscription, permissionErr error) { permissionErrors <- permissionErr }),
			)
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(runtimeConnection.Close)
			received := make(chan struct{}, 1)
			if plane.roundTrip {
				allowed, subscribeErr := runtimeConnection.Subscribe(plane.allowedSubject, func(*nats.Msg) { received <- struct{}{} })
				if subscribeErr != nil {
					t.Fatalf("allowed subscribe: %v", subscribeErr)
				}
				t.Cleanup(func() { _ = allowed.Unsubscribe() })
				if err := runtimeConnection.Flush(); err != nil {
					t.Fatal(err)
				}
			}
			if err := runtimeConnection.Publish(plane.allowedSubject, []byte(`{"fixture":true}`)); err != nil {
				t.Fatalf("allowed publish: %v", err)
			}
			if err := runtimeConnection.Flush(); err != nil {
				t.Fatal(err)
			}
			if plane.roundTrip {
				select {
				case <-received:
				case <-time.After(2 * time.Second):
					t.Fatal("allowed runtime data subject did not round-trip")
				}
			}

			assertMainPermissionDenied(t, runtimeConnection, permissionErrors, "wildcard subscription", func() error {
				_, subscribeErr := runtimeConnection.Subscribe(">", func(*nats.Msg) {})
				return subscribeErr
			})
			assertMainPermissionDenied(t, runtimeConnection, permissionErrors, "JetStream administration", func() error {
				return runtimeConnection.Publish("$JS.API.STREAM.CREATE.FORGED", []byte(`{"name":"FORGED","subjects":["forged.>"]}`))
			})
			assertMainPermissionDenied(t, runtimeConnection, permissionErrors, "Audit ACK", func() error {
				return runtimeConnection.Publish(plane.ack, nil)
			})
			assertMainPermissionDenied(t, runtimeConnection, permissionErrors, "Audit delivery", func() error {
				_, subscribeErr := runtimeConnection.Subscribe(plane.delivery, func(*nats.Msg) {})
				return subscribeErr
			})
		})
	}
}

func TestApplicationModelConsumersUseScopedCredentialsAndLegacyTokenIsRejected(t *testing.T) {
	const password = "0123456789abcdef0123456789abcdef"
	for _, envName := range []string{
		"MODEL_NATS_RUNTIME_PASSWORD", "MODEL_GATEWAY_NATS_PASSWORD", "MODEL_SESSION_CORE_NATS_PASSWORD",
		"AUDIT_MODEL_NATS_PASSWORD", "MODEL_NATS_PROVISIONER_PASSWORD",
		"APPLICATION_CONVEX_MODEL_NATS_PASSWORD", "APPLICATION_INSIGHT_MODEL_NATS_PASSWORD",
	} {
		t.Setenv(envName, strconv.Quote(password))
	}
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test path")
	}
	controlRoot := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
	options, err := natsserver.ProcessConfigFile(filepath.Join(controlRoot, "..", "Model Plane", "deploy", "nats.conf"))
	if err != nil {
		t.Fatal(err)
	}
	options.Port = -1
	options.HTTPPort = -1
	options.StoreDir = t.TempDir()
	instance, err := natsserver.NewServer(options)
	if err != nil {
		t.Fatal(err)
	}
	go instance.Start()
	if !instance.ReadyForConnections(10 * time.Second) {
		t.Fatal("Model broker did not start")
	}
	defer func() { instance.Shutdown(); instance.WaitForShutdown() }()

	if legacy, connectErr := nats.Connect(instance.ClientURL(), nats.Token("legacy-model-token"), nats.Timeout(time.Second)); connectErr == nil {
		legacy.Close()
		t.Fatal("removed Model token authentication was still accepted")
	}
	topology, err := nats.Connect(instance.ClientURL(), nats.UserInfo("observability-provisioner-model", password), nats.CustomInboxPrefix("_INBOX.PROVISIONER_MODEL"))
	if err != nil {
		t.Fatal(err)
	}
	defer topology.Close()
	topologyJS, err := topology.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if err := provisioner.Provision(context.Background(), topologyJS, provisioner.Bus{Name: "model", Plane: "model"}); err != nil {
		t.Fatal(err)
	}
	producer, err := nats.Connect(instance.ClientURL(), nats.UserInfo("model-runtime", password), nats.CustomInboxPrefix("_INBOX.MODEL_RUNTIME"))
	if err != nil {
		t.Fatal(err)
	}
	defer producer.Close()
	convex, err := nats.Connect(instance.ClientURL(), nats.UserInfo("application-convex-model", password), nats.CustomInboxPrefix("_INBOX.APPLICATION_CONVEX_MODEL"))
	if err != nil {
		t.Fatal(err)
	}
	defer convex.Close()
	received := make(chan struct{}, 1)
	if _, err := convex.Subscribe("mp.v1.run.*.event", func(*nats.Msg) { received <- struct{}{} }); err != nil {
		t.Fatal(err)
	}
	if err := convex.Flush(); err != nil {
		t.Fatal(err)
	}
	insight, err := nats.Connect(instance.ClientURL(), nats.UserInfo("application-insight-model", password), nats.CustomInboxPrefix("_INBOX.APPLICATION_INSIGHT_MODEL"))
	if err != nil {
		t.Fatal(err)
	}
	defer insight.Close()
	insightJS, err := insight.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	insightReceived := make(chan struct{}, 1)
	insightSub, err := insightJS.QueueSubscribe(
		"mp.v1.run.*.event", provisioner.InsightRunConsumerName,
		func(message *nats.Msg) { _ = message.Ack(); insightReceived <- struct{}{} },
		nats.Bind(provisioner.ModelRunEventsStreamName, provisioner.InsightRunConsumerName),
	)
	if err != nil {
		t.Fatalf("bind scoped Insight consumer: %v", err)
	}
	defer func() { _ = insightSub.Unsubscribe() }()
	if err := producer.Publish("mp.v1.run.run-1.event", []byte(`{"event_id":"event-1"}`)); err != nil {
		t.Fatal(err)
	}
	select {
	case <-received:
	case <-time.After(2 * time.Second):
		t.Fatal("scoped Application-to-Model consumer did not receive allowed run event")
	}
	select {
	case <-insightReceived:
	case <-time.After(2 * time.Second):
		t.Fatal("scoped Insight durable consumer did not receive allowed run event")
	}
}

func TestModelAuditProducersUseDistinctServicePrincipals(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test path")
	}
	controlRoot := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
	modelRoot := filepath.Join(controlRoot, "..", "Model Plane")
	composeBytes, err := os.ReadFile(filepath.Join(modelRoot, "deploy", "docker-compose.yml"))
	if err != nil {
		t.Fatal(err)
	}
	brokerBytes, err := os.ReadFile(filepath.Join(modelRoot, "deploy", "nats.conf"))
	if err != nil {
		t.Fatal(err)
	}
	compose := string(composeBytes)
	broker := string(brokerBytes)
	principals := []struct {
		service      string
		user         string
		password     string
		inbox        string
		auditSubject string
		source       string
	}{
		{
			service: "model-gateway", user: "model-gateway-runtime", password: "MODEL_GATEWAY_NATS_PASSWORD",
			inbox:  "_INBOX.MODEL_GATEWAY_RUNTIME.>",
			source: filepath.Join(modelRoot, "rust", "services", "model-gateway", "src", "nats_connection.rs"),
		},
		{
			service: "session-core", user: "session-core-runtime", password: "MODEL_SESSION_CORE_NATS_PASSWORD",
			inbox: "_INBOX.SESSION_CORE_RUNTIME.>", auditSubject: "velion.audit.v2.model.session-core.>",
			source: filepath.Join(modelRoot, "rust", "services", "session-core", "src", "nats_connection.rs"),
		},
	}
	for _, principal := range principals {
		block, blockErr := composeServiceBlock(compose, principal.service)
		if blockErr != nil {
			t.Fatal(blockErr)
		}
		if !strings.Contains(block, "NATS_USER: "+principal.user) ||
			!strings.Contains(block, "NATS_PASSWORD: ${"+principal.password+":?") {
			t.Fatalf("%s is not wired to scoped Model principal %s", principal.service, principal.user)
		}
		for _, forbidden := range []string{"MODEL_NATS_RUNTIME_PASSWORD", "NATS_AUTH_TOKEN", "NATS_ALLOW_TOKEN_FALLBACK"} {
			if strings.Contains(block, forbidden) {
				t.Fatalf("%s still receives shared or token Model credential %q", principal.service, forbidden)
			}
		}
		userBlock, blockErr := natsUserConfigBlock(broker, principal.user)
		if blockErr != nil {
			t.Fatal(blockErr)
		}
		for _, required := range []string{"password: $" + principal.password, principal.inbox} {
			if !strings.Contains(userBlock, required) {
				t.Fatalf("Model principal %s missing capability %q", principal.user, required)
			}
		}
		if principal.auditSubject != "" && !strings.Contains(userBlock, principal.auditSubject) {
			t.Fatalf("Model principal %s missing audit capability %q", principal.user, principal.auditSubject)
		}
		for _, forbidden := range []string{`"_INBOX.>"`, `publish: ">"`, `subscribe: ">"`} {
			if strings.Contains(userBlock, forbidden) {
				t.Fatalf("Model principal %s retains broad capability %q", principal.user, forbidden)
			}
		}
		sourceBytes, readErr := os.ReadFile(principal.source)
		if readErr != nil {
			t.Fatal(readErr)
		}
		if !strings.Contains(string(sourceBytes), strings.TrimSuffix(principal.inbox, ".>")) ||
			strings.Contains(string(sourceBytes), "_INBOX.MODEL_RUNTIME") {
			t.Fatalf("%s connector does not pin its scoped reply inbox", principal.service)
		}
	}

	gatewayBlock, err := natsUserConfigBlock(broker, "model-gateway-runtime")
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(gatewayBlock, "velion.audit.v2.model.") || strings.Contains(gatewayBlock, "session-core-tools") {
		t.Fatal("model-gateway principal can cross the session audit or durable-consumer boundary")
	}
	sessionBlock, err := natsUserConfigBlock(broker, "session-core-runtime")
	if err != nil {
		t.Fatal(err)
	}
	for _, required := range []string{
		"$JS.API.STREAM.INFO.TOOLS_COMPLETIONS",
		"$JS.API.STREAM.INFO.MP_ORCHESTRATION_EVENTS",
		"$JS.API.CONSUMER.INFO.TOOLS_COMPLETIONS.session-core-tools",
		"$JS.API.CONSUMER.INFO.MP_ORCHESTRATION_EVENTS.session-core-orchestration",
		"$JS.API.CONSUMER.MSG.NEXT.TOOLS_COMPLETIONS.session-core-tools",
		"$JS.API.CONSUMER.MSG.NEXT.MP_ORCHESTRATION_EVENTS.session-core-orchestration",
		"$JS.ACK.TOOLS_COMPLETIONS.session-core-tools.>",
	} {
		if !strings.Contains(sessionBlock, required) {
			t.Fatalf("session-core principal missing fixed-consumer capability %q", required)
		}
	}
	if strings.Contains(sessionBlock, "velion.audit.v2.model.model-gateway.>") {
		t.Fatal("session-core principal can publish model-gateway audit events")
	}
	sharedBlock, err := natsUserConfigBlock(broker, "model-runtime")
	if err != nil {
		t.Fatal(err)
	}
	for _, forbidden := range []string{
		"velion.audit.v2.model.model-gateway.>", "velion.audit.v2.model.session-core.>",
		"session-core-tools", "session-core-orchestration",
	} {
		if strings.Contains(sharedBlock, forbidden) {
			t.Fatalf("shared model-runtime retains producer-specific capability %q", forbidden)
		}
	}
}

func TestModelAuditProducerACLsEnforceNamespacesAndSessionConsumer(t *testing.T) {
	passwords := map[string]string{
		"MODEL_NATS_RUNTIME_PASSWORD":             "shared-runtime-0123456789abcdef0123456789",
		"MODEL_GATEWAY_NATS_PASSWORD":             "gateway-runtime-0123456789abcdef01234567",
		"MODEL_SESSION_CORE_NATS_PASSWORD":        "session-runtime-0123456789abcdef01234567",
		"AUDIT_MODEL_NATS_PASSWORD":               "audit-consumer-0123456789abcdef012345678",
		"MODEL_NATS_PROVISIONER_PASSWORD":         "topology-admin-0123456789abcdef01234567",
		"APPLICATION_CONVEX_MODEL_NATS_PASSWORD":  "convex-reader-0123456789abcdef012345678",
		"APPLICATION_INSIGHT_MODEL_NATS_PASSWORD": "insight-reader-0123456789abcdef01234567",
	}
	for envName, password := range passwords {
		t.Setenv(envName, strconv.Quote(password))
	}
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test path")
	}
	controlRoot := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
	options, err := natsserver.ProcessConfigFile(filepath.Join(controlRoot, "..", "Model Plane", "deploy", "nats.conf"))
	if err != nil {
		t.Fatal(err)
	}
	options.Port = -1
	options.HTTPPort = -1
	options.StoreDir = t.TempDir()
	instance, err := natsserver.NewServer(options)
	if err != nil {
		t.Fatal(err)
	}
	go instance.Start()
	if !instance.ReadyForConnections(10 * time.Second) {
		t.Fatal("Model broker did not start")
	}
	t.Cleanup(func() { instance.Shutdown(); instance.WaitForShutdown() })

	connect := func(user, password, inbox string) (*nats.Conn, <-chan error) {
		permissionErrors := make(chan error, 16)
		connection, connectErr := nats.Connect(instance.ClientURL(),
			nats.UserInfo(user, password),
			nats.CustomInboxPrefix(inbox),
			nats.ErrorHandler(func(_ *nats.Conn, _ *nats.Subscription, permissionErr error) {
				permissionErrors <- permissionErr
			}),
		)
		if connectErr != nil {
			t.Fatalf("connect %s: %v", user, connectErr)
		}
		t.Cleanup(connection.Close)
		return connection, permissionErrors
	}

	provisioning, _ := connect("observability-provisioner-model", passwords["MODEL_NATS_PROVISIONER_PASSWORD"], "_INBOX.PROVISIONER_MODEL")
	provisioningJS, err := provisioning.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if err := provisioner.Provision(context.Background(), provisioningJS, provisioner.Bus{Name: "model", Plane: "model"}); err != nil {
		t.Fatal(err)
	}

	gateway, gatewayErrors := connect("model-gateway-runtime", passwords["MODEL_GATEWAY_NATS_PASSWORD"], "_INBOX.MODEL_GATEWAY_RUNTIME")
	session, sessionErrors := connect("session-core-runtime", passwords["MODEL_SESSION_CORE_NATS_PASSWORD"], "_INBOX.SESSION_CORE_RUNTIME")
	shared, sharedErrors := connect("model-runtime", passwords["MODEL_NATS_RUNTIME_PASSWORD"], "_INBOX.MODEL_RUNTIME")
	sessionPublishJS, jsErr := session.JetStream(nats.MaxWait(2 * time.Second))
	if jsErr != nil {
		t.Fatal(jsErr)
	}
	if _, publishErr := sessionPublishJS.Publish("velion.audit.v2.model.session-core.fixture", []byte(`{"event_id":"fixture"}`)); publishErr != nil {
		t.Fatalf("session-core audit namespace did not receive a JetStream PubAck: %v", publishErr)
	}
	assertMainPermissionDenied(t, gateway, gatewayErrors, "gateway cross-producer audit publish", func() error {
		return gateway.Publish("velion.audit.v2.model.session-core.forged", nil)
	})
	assertMainPermissionDenied(t, gateway, gatewayErrors, "gateway legacy self-producer audit publish", func() error {
		return gateway.Publish("velion.audit.v2.model.model-gateway.forged", nil)
	})
	assertMainPermissionDenied(t, session, sessionErrors, "session cross-producer audit publish", func() error {
		return session.Publish("velion.audit.v2.model.model-gateway.forged", nil)
	})
	for _, subject := range []string{
		"velion.audit.v2.model.model-gateway.forged",
		"velion.audit.v2.model.session-core.forged",
	} {
		assertMainPermissionDenied(t, shared, sharedErrors, "shared runtime audit publish", func() error {
			return shared.Publish(subject, nil)
		})
	}

	sessionJS, err := session.JetStream(nats.MaxWait(2 * time.Second))
	if err != nil {
		t.Fatal(err)
	}
	toolConsumer, err := sessionJS.PullSubscribe("tools.completions.*", provisioner.SessionToolsConsumerName,
		nats.Bind(provisioner.ModelToolsStreamName, provisioner.SessionToolsConsumerName))
	if err != nil {
		t.Fatalf("bind session-core fixed tool consumer: %v", err)
	}
	t.Cleanup(func() { _ = toolConsumer.Unsubscribe() })
	if err := shared.Publish("tools.completions.fixture", []byte(`{"event_id":"tool-fixture"}`)); err != nil {
		t.Fatal(err)
	}
	if err := shared.Flush(); err != nil {
		t.Fatal(err)
	}
	messages, err := toolConsumer.Fetch(1, nats.MaxWait(2*time.Second))
	if err != nil || len(messages) != 1 {
		t.Fatalf("session-core fixed tool consumer fetch = %d messages, %v", len(messages), err)
	}
	if err := messages[0].AckSync(); err != nil {
		t.Fatalf("session-core fixed tool consumer ack: %v", err)
	}
	orchestrationConsumer, err := sessionJS.PullSubscribe("mp.v1.orchestration.>", provisioner.SessionOrchestrationConsumerName,
		nats.Bind(provisioner.ModelOrchestrationStreamName, provisioner.SessionOrchestrationConsumerName))
	if err != nil {
		t.Fatalf("bind session-core fixed orchestration consumer: %v", err)
	}
	t.Cleanup(func() { _ = orchestrationConsumer.Unsubscribe() })
	if err := shared.Publish("mp.v1.orchestration.fixture", []byte(`{"event_id":"orchestration-fixture"}`)); err != nil {
		t.Fatal(err)
	}
	if err := shared.Flush(); err != nil {
		t.Fatal(err)
	}
	orchestrationMessages, err := orchestrationConsumer.Fetch(1, nats.MaxWait(2*time.Second))
	if err != nil || len(orchestrationMessages) != 1 {
		t.Fatalf("session-core fixed orchestration consumer fetch = %d messages, %v", len(orchestrationMessages), err)
	}
	sharedJS, err := shared.JetStream(nats.MaxWait(time.Second))
	if err != nil {
		t.Fatal(err)
	}
	assertMainPermissionDenied(t, shared, sharedErrors, "shared runtime session consumer bind", func() error {
		_, bindErr := sharedJS.PullSubscribe("tools.completions.*", provisioner.SessionToolsConsumerName,
			nats.Bind(provisioner.ModelToolsStreamName, provisioner.SessionToolsConsumerName))
		return bindErr
	})

	for _, user := range []string{"model-gateway-runtime", "session-core-runtime"} {
		if forged, connectErr := nats.Connect(instance.ClientURL(),
			nats.UserInfo(user, passwords["MODEL_NATS_RUNTIME_PASSWORD"]), nats.Timeout(time.Second)); connectErr == nil {
			forged.Close()
			t.Fatalf("%s accepted the shared model-runtime password", user)
		}
	}
	if legacy, connectErr := nats.Connect(instance.ClientURL(), nats.Token("legacy-model-token"), nats.Timeout(time.Second)); connectErr == nil {
		legacy.Close()
		t.Fatal("legacy shared Model token authentication was accepted")
	}
}

func TestApplicationLocalBrokerUsesPerServicePrincipals(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test path")
	}
	controlRoot := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
	applicationRoot := filepath.Join(controlRoot, "..", "Application Plane")
	composeBytes, err := os.ReadFile(filepath.Join(applicationRoot, "docker-compose.yml"))
	if err != nil {
		t.Fatal(err)
	}
	brokerBytes, err := os.ReadFile(filepath.Join(applicationRoot, "nats.conf"))
	if err != nil {
		t.Fatal(err)
	}
	compose := string(composeBytes)
	broker := string(brokerBytes)
	principals := []struct {
		service  string
		user     string
		password string
	}{
		{service: "conversation-core-go", user: "application-conversation", password: "APPLICATION_CONVERSATION_NATS_PASSWORD"},
		{service: "social-core", user: "application-social", password: "APPLICATION_SOCIAL_NATS_PASSWORD"},
		{service: "insight-core", user: "application-insight", password: "APPLICATION_INSIGHT_NATS_PASSWORD"},
		{service: "leads-core", user: "application-leads", password: "APPLICATION_LEADS_NATS_PASSWORD"},
		{service: "notification-core", user: "application-notification", password: "APPLICATION_NOTIFICATION_NATS_PASSWORD"},
	}
	for _, principal := range principals {
		block, blockErr := composeServiceBlock(compose, principal.service)
		if blockErr != nil {
			t.Fatal(blockErr)
		}
		if !strings.Contains(block, "NATS_USER: "+principal.user) ||
			!strings.Contains(block, "NATS_PASSWORD: ${"+principal.password+":?") {
			t.Fatalf("%s is not wired to scoped local principal %s", principal.service, principal.user)
		}
		for _, forbidden := range []string{"\n      NATS_TOKEN:", "\n      VELION_NATS_TOKEN:", "APPLICATION_NATS_RUNTIME_PASSWORD"} {
			if strings.Contains(block, forbidden) {
				t.Fatalf("%s still receives shared local broker credential %q", principal.service, forbidden)
			}
		}
		userBlock, blockErr := natsUserConfigBlock(broker, principal.user)
		if blockErr != nil {
			t.Fatal(blockErr)
		}
		for _, forbidden := range []string{"$JS.API.STREAM.>", "$JS.API.CONSUMER.>", `"_INBOX.>"`, `publish: ">"`, `subscribe: ">"`} {
			if strings.Contains(userBlock, forbidden) {
				t.Fatalf("Application principal %s retains broad capability %q", principal.user, forbidden)
			}
		}
	}
	if strings.Contains(broker, `user: "application-runtime"`) || strings.Contains(compose, "APPLICATION_NATS_RUNTIME_PASSWORD") {
		t.Fatal("shared Application runtime principal remains in release configuration")
	}
}

func TestConvexControlProjectionUsesScopedPreprovisionedConsumer(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test path")
	}
	controlRoot := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
	applicationRoot := filepath.Join(controlRoot, "..", "Application Plane")
	applicationComposeBytes, err := os.ReadFile(filepath.Join(applicationRoot, "docker-compose.yml"))
	if err != nil {
		t.Fatal(err)
	}
	standaloneComposeBytes, err := os.ReadFile(filepath.Join(applicationRoot, "convex-core", "docker-compose.yml"))
	if err != nil {
		t.Fatal(err)
	}
	subscriberBytes, err := os.ReadFile(filepath.Join(applicationRoot, "convex-core", "nats-subscriber.js"))
	if err != nil {
		t.Fatal(err)
	}
	brokerBytes, err := os.ReadFile(filepath.Join(controlRoot, "control-shared-nats.conf"))
	if err != nil {
		t.Fatal(err)
	}

	for _, compose := range []string{string(applicationComposeBytes), string(standaloneComposeBytes)} {
		block, blockErr := composeServiceBlock(compose, "convex-subscriber")
		if blockErr != nil {
			t.Fatal(blockErr)
		}
		for _, required := range []string{
			"NATS_URL: nats://control-shared-nats:4222",
			"NATS_USER: application-convex-control",
			"APPLICATION_CONVEX_CONTROL_NATS_PASSWORD",
		} {
			if !strings.Contains(block, required) {
				t.Fatalf("Convex scoped Control wiring missing %q", required)
			}
		}
		for _, forbidden := range []string{"NATS_TOKEN", "VELION_NATS_TOKEN"} {
			if strings.Contains(block, forbidden) {
				t.Fatalf("Convex still receives token credential %q", forbidden)
			}
		}
	}

	subscriber := string(subscriberBytes)
	for _, forbidden := range []string{"NATS_TOKEN", "VELION_NATS_TOKEN", "jetstreamManager", "streams.add"} {
		if strings.Contains(subscriber, forbidden) {
			t.Fatalf("Convex runtime retains token/admin path %q", forbidden)
		}
	}
	if !strings.Contains(subscriber, "options.bind(CONTROL_PLANE_STREAM") {
		t.Fatal("Convex runtime does not bind a pre-provisioned Control consumer")
	}
	userBlock, err := natsUserConfigBlock(string(brokerBytes), "application-convex-control")
	if err != nil {
		t.Fatal(err)
	}
	for _, forbidden := range []string{"$JS.API.STREAM.>", "$JS.API.CONSUMER.>", `publish: ">"`, `subscribe: ">"`} {
		if strings.Contains(userBlock, forbidden) {
			t.Fatalf("Convex Control principal retains broad capability %q", forbidden)
		}
	}

	notificationBlock, err := composeServiceBlock(string(applicationComposeBytes), "notification-core")
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(notificationBlock, "SHARED_NATS_TOKEN") || strings.Contains(notificationBlock, "VELION_NATS_TOKEN") {
		t.Fatal("disabled Notification shared-bus consumer still receives a token")
	}
}

func TestFreshControlSharedBrokerAllowsOnlyThePreprovisionedConvexProjection(t *testing.T) {
	const password = "0123456789abcdef0123456789abcdef"
	for _, envName := range []string{
		"AUTH_SHARED_NATS_PASSWORD", "USER_SHARED_NATS_PASSWORD",
		"ORG_SHARED_NATS_PASSWORD", "BILLING_SHARED_NATS_PASSWORD",
		"SESSION_SHARED_NATS_PASSWORD", "APPLICATION_CONVEX_CONTROL_NATS_PASSWORD",
		"DOCUMENTS_GDPR_NATS_PASSWORD",
		"CONTROL_SHARED_BRIDGE_PASSWORD", "CONTROL_SHARED_NATS_PROVISIONER_PASSWORD",
	} {
		t.Setenv(envName, strconv.Quote(password))
	}

	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test path")
	}
	controlRoot := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
	options, err := natsserver.ProcessConfigFile(filepath.Join(controlRoot, "control-shared-nats.conf"))
	if err != nil {
		t.Fatal(err)
	}
	options.Port = -1
	options.HTTPPort = -1
	options.StoreDir = t.TempDir()
	instance, err := natsserver.NewServer(options)
	if err != nil {
		t.Fatal(err)
	}
	go instance.Start()
	if !instance.ReadyForConnections(10 * time.Second) {
		t.Fatal("Control shared broker did not start")
	}
	t.Cleanup(func() { instance.Shutdown(); instance.WaitForShutdown() })

	topology, err := nats.Connect(instance.ClientURL(),
		nats.UserInfo("control-shared-provisioner", password),
		nats.CustomInboxPrefix("_INBOX.PROVISIONER_SHARED"),
	)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(topology.Close)
	topologyJS, err := topology.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if err := provisioner.Provision(context.Background(), topologyJS, provisioner.Bus{Name: "shared-control", Plane: "shared"}); err != nil {
		t.Fatal(err)
	}

	producer, err := nats.Connect(instance.ClientURL(),
		nats.UserInfo("auth-core-shared", password),
		nats.CustomInboxPrefix("_INBOX.AUTH_SHARED"),
	)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(producer.Close)

	permissionErrors := make(chan error, 8)
	convex, err := nats.Connect(instance.ClientURL(),
		nats.UserInfo("application-convex-control", password),
		nats.CustomInboxPrefix("_INBOX.APPLICATION_CONVEX_CONTROL"),
		nats.ErrorHandler(func(_ *nats.Conn, _ *nats.Subscription, permissionErr error) {
			permissionErrors <- permissionErr
		}),
	)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(convex.Close)
	convexJS, err := convex.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	received := make(chan struct{}, 1)
	subscription, err := convexJS.QueueSubscribe(
		"aqencia.controlplane.org.member_changed",
		"convex-org-member-changed-v2",
		func(message *nats.Msg) {
			_ = message.Ack()
			received <- struct{}{}
		},
		nats.Bind(provisioner.ControlSharedStreamName, "convex-org-member-changed-v2"),
	)
	if err != nil {
		t.Fatalf("bind scoped Convex consumer: %v", err)
	}
	t.Cleanup(func() { _ = subscription.Unsubscribe() })
	if err := producer.Publish("aqencia.controlplane.org.member_changed", []byte(`{"org_id":"org-1","user_id":"user-1","revision":2}`)); err != nil {
		t.Fatal(err)
	}
	if err := producer.Flush(); err != nil {
		t.Fatal(err)
	}
	select {
	case <-received:
	case <-time.After(2 * time.Second):
		t.Fatal("scoped Convex consumer did not receive the authoritative Control event")
	}

	orgPermissionErrors := make(chan error, 2)
	orgProducer, err := nats.Connect(instance.ClientURL(),
		nats.UserInfo("org-core-shared", password),
		nats.CustomInboxPrefix("_INBOX.ORG_SHARED"),
		nats.ErrorHandler(func(_ *nats.Conn, _ *nats.Subscription, permissionErr error) {
			orgPermissionErrors <- permissionErr
		}),
	)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(orgProducer.Close)
	assertMainPermissionDenied(t, orgProducer, orgPermissionErrors, "Org canonical projection forgery", func() error {
		return orgProducer.Publish("aqencia.controlplane.org.member_changed", []byte(`{"forged":true}`))
	})

	if err := convex.Publish(provisioner.ConvexControlDLQSubject, []byte(`{"event_id":"event-1"}`)); err != nil {
		t.Fatalf("publish bounded Convex DLQ event: %v", err)
	}
	if err := convex.Flush(); err != nil {
		t.Fatal(err)
	}
	assertMainPermissionDenied(t, convex, permissionErrors, "JetStream administration", func() error {
		return convex.Publish("$JS.API.STREAM.CREATE.FORGED", []byte(`{"name":"FORGED","subjects":["forged.>"]}`))
	})
	assertMainPermissionDenied(t, convex, permissionErrors, "Control event forgery", func() error {
		return convex.Publish("aqencia.controlplane.org.member_removed", []byte(`{"forged":true}`))
	})

	userProducer, err := nats.Connect(instance.ClientURL(),
		nats.UserInfo("user-core-shared", password),
		nats.CustomInboxPrefix("_INBOX.USER_SHARED"),
	)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(userProducer.Close)
	documentsPermissionErrors := make(chan error, 4)
	documents, err := nats.Connect(instance.ClientURL(),
		nats.UserInfo("documents-api-gdpr", password),
		nats.CustomInboxPrefix("_INBOX.DOCUMENTS_GDPR"),
		nats.ErrorHandler(func(_ *nats.Conn, _ *nats.Subscription, permissionErr error) {
			documentsPermissionErrors <- permissionErr
		}),
	)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(documents.Close)
	documentsJS, err := documents.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	gdprReceived := make(chan struct{}, 1)
	gdprSubscription, err := documentsJS.QueueSubscribe(
		provisioner.GDPRErasureRequestedSubject,
		provisioner.DocumentsGDPRConsumerName,
		func(message *nats.Msg) {
			_ = message.Ack()
			gdprReceived <- struct{}{}
		},
		nats.Bind(provisioner.ControlSharedStreamName, provisioner.DocumentsGDPRConsumerName),
		nats.ManualAck(),
	)
	if err != nil {
		t.Fatalf("bind scoped documents GDPR consumer: %v", err)
	}
	t.Cleanup(func() { _ = gdprSubscription.Unsubscribe() })
	if err := userProducer.Publish(provisioner.GDPRErasureRequestedSubject, []byte(`{"event_id":"gdpr:fanout:test","subject_type":"user","subject_id":"user-1","org_id":"org-1"}`)); err != nil {
		t.Fatal(err)
	}
	if err := userProducer.Flush(); err != nil {
		t.Fatal(err)
	}
	select {
	case <-gdprReceived:
	case <-time.After(2 * time.Second):
		t.Fatal("scoped documents GDPR consumer did not receive the durable erasure event")
	}
	if _, err := documentsJS.Publish(provisioner.GDPRErasureDLQSubject, []byte(`{"event_id":"gdpr:dlq:test"}`)); err != nil {
		t.Fatalf("publish scoped documents GDPR DLQ: %v", err)
	}
	assertMainPermissionDenied(t, documents, documentsPermissionErrors, "GDPR request forgery", func() error {
		return documents.Publish(provisioner.GDPRErasureRequestedSubject, []byte(`{"forged":true}`))
	})
	assertMainPermissionDenied(t, documents, documentsPermissionErrors, "documents JetStream administration", func() error {
		return documents.Publish("$JS.API.STREAM.CREATE.FORGED", []byte(`{"name":"FORGED"}`))
	})
}

func TestFreshApplicationBrokerSupportsScopedActiveClients(t *testing.T) {
	const password = "0123456789abcdef0123456789abcdef"
	for _, envName := range []string{
		"APPLICATION_CONVERSATION_NATS_PASSWORD", "APPLICATION_SOCIAL_NATS_PASSWORD",
		"APPLICATION_INSIGHT_NATS_PASSWORD", "APPLICATION_LEADS_NATS_PASSWORD",
		"APPLICATION_NOTIFICATION_NATS_PASSWORD", "AUDIT_APPLICATION_NATS_PASSWORD",
		"APPLICATION_NATS_PROVISIONER_PASSWORD",
	} {
		t.Setenv(envName, strconv.Quote(password))
	}
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test path")
	}
	controlRoot := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
	options, err := natsserver.ProcessConfigFile(filepath.Join(controlRoot, "..", "Application Plane", "nats.conf"))
	if err != nil {
		t.Fatal(err)
	}
	options.Port = -1
	options.HTTPPort = -1
	options.StoreDir = t.TempDir()
	instance, err := natsserver.NewServer(options)
	if err != nil {
		t.Fatal(err)
	}
	go instance.Start()
	if !instance.ReadyForConnections(10 * time.Second) {
		t.Fatal("Application broker did not start")
	}
	t.Cleanup(func() { instance.Shutdown(); instance.WaitForShutdown() })

	for name, option := range map[string]nats.Option{
		"legacy token":   nats.Token("legacy-application-token"),
		"shared runtime": nats.UserInfo("application-runtime", password),
	} {
		connection, connectErr := nats.Connect(instance.ClientURL(), option, nats.Timeout(time.Second))
		if connectErr == nil {
			connection.Close()
			t.Fatalf("removed %s credential was accepted", name)
		}
	}

	connect := func(user, inbox string, extra ...nats.Option) *nats.Conn {
		opts := []nats.Option{nats.UserInfo(user, password), nats.CustomInboxPrefix(inbox)}
		opts = append(opts, extra...)
		connection, connectErr := nats.Connect(instance.ClientURL(), opts...)
		if connectErr != nil {
			t.Fatalf("connect %s: %v", user, connectErr)
		}
		t.Cleanup(connection.Close)
		return connection
	}
	provisionerConnection := connect("observability-provisioner-application", "_INBOX.PROVISIONER_APPLICATION")
	provisionerJS, err := provisionerConnection.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if err := provisioner.Provision(context.Background(), provisionerJS, provisioner.Bus{Name: "application", Plane: "application"}); err != nil {
		t.Fatalf("provision Application broker: %v", err)
	}

	conversation := connect("application-conversation", "_INBOX.APPLICATION_CONVERSATION")
	socialPermissionErrors := make(chan error, 4)
	social := connect("application-social", "_INBOX.APPLICATION_SOCIAL", nats.ErrorHandler(func(_ *nats.Conn, _ *nats.Subscription, permissionErr error) {
		socialPermissionErrors <- permissionErr
	}))
	insight := connect("application-insight", "_INBOX.APPLICATION_INSIGHT")
	leads := connect("application-leads", "_INBOX.APPLICATION_LEADS")
	notification := connect("application-notification", "_INBOX.APPLICATION_NOTIFICATION")

	conversationJS, _ := conversation.JetStream(nats.MaxWait(2 * time.Second))
	insightJS, _ := insight.JetStream(nats.MaxWait(2 * time.Second))
	reviewed := make(chan struct{}, 1)
	reviewedSub, err := conversationJS.QueueSubscribe(
		"velion.application.conversation.ai_action.reviewed", provisioner.ConversationAIActionConsumerName,
		func(message *nats.Msg) { _ = message.Ack(); reviewed <- struct{}{} },
		nats.Bind(provisioner.ApplicationEventsStreamName, provisioner.ConversationAIActionConsumerName),
	)
	if err != nil {
		t.Fatalf("bind scoped conversation consumer: %v", err)
	}
	t.Cleanup(func() { _ = reviewedSub.Unsubscribe() })
	metrics := make(chan struct{}, 1)
	metricSub, err := insightJS.QueueSubscribe(
		"velion.application.>", provisioner.InsightMetricConsumerName,
		func(message *nats.Msg) { _ = message.Ack(); metrics <- struct{}{} },
		nats.Bind(provisioner.ApplicationEventsStreamName, provisioner.InsightMetricConsumerName),
	)
	if err != nil {
		t.Fatalf("bind scoped Insight consumer: %v", err)
	}
	t.Cleanup(func() { _ = metricSub.Unsubscribe() })
	if _, err := conversationJS.Publish("velion.application.conversation.ai_action.reviewed", []byte(`{"fixture":true}`)); err != nil {
		t.Fatalf("conversation publish: %v", err)
	}
	for name, delivered := range map[string]<-chan struct{}{"conversation": reviewed, "insight": metrics} {
		select {
		case <-delivered:
		case <-time.After(2 * time.Second):
			t.Fatalf("%s fixed consumer did not receive Application event", name)
		}
	}
	for _, publish := range []struct {
		connection *nats.Conn
		subject    string
	}{
		{social, "velion.application.social.fixture"},
		{leads, "velion.audit.v2.application.leads-core.fixture"},
		{notification, "velion.application.notification.fixture"},
	} {
		if err := publish.connection.Publish(publish.subject, []byte(`{"fixture":true}`)); err != nil {
			t.Fatalf("scoped publish %s: %v", publish.subject, err)
		}
	}
	assertMainPermissionDenied(t, social, socialPermissionErrors, "cross-service publish", func() error {
		return social.Publish("velion.application.notification.forged", nil)
	})
	if _, err := conversationJS.AddStream(&nats.StreamConfig{Name: "FORGED", Subjects: []string{"forged.>"}}); err == nil {
		t.Fatal("Application runtime principal administered an unrelated stream")
	}
}

func TestScopedBrokerConfigsParse(t *testing.T) {
	rawPassword := "1e99e912345678901234567890123456"
	for _, name := range []string{
		"AUTH_NATS_PASSWORD", "USER_NATS_PASSWORD", "ORG_NATS_PASSWORD", "BILLING_NATS_PASSWORD", "SESSION_NATS_PASSWORD",
		"AUDIT_CONTROL_NATS_PASSWORD", "CONTROL_NATS_PROVISIONER_PASSWORD",
		"MODEL_NATS_RUNTIME_PASSWORD", "MODEL_GATEWAY_NATS_PASSWORD", "MODEL_SESSION_CORE_NATS_PASSWORD",
		"AUDIT_MODEL_NATS_PASSWORD", "MODEL_NATS_PROVISIONER_PASSWORD",
		"APPLICATION_CONVEX_MODEL_NATS_PASSWORD", "APPLICATION_INSIGHT_MODEL_NATS_PASSWORD",
		"APPLICATION_CONVERSATION_NATS_PASSWORD", "APPLICATION_SOCIAL_NATS_PASSWORD", "APPLICATION_INSIGHT_NATS_PASSWORD",
		"APPLICATION_LEADS_NATS_PASSWORD", "APPLICATION_NOTIFICATION_NATS_PASSWORD",
		"AUDIT_APPLICATION_NATS_PASSWORD", "APPLICATION_NATS_PROVISIONER_PASSWORD",
	} {
		// NATS resolves an environment variable as configuration syntax. Compose
		// wraps only the broker's copy in quotes so arbitrary generated values
		// (including numeric/exponent-like prefixes) parse as the exact string.
		t.Setenv(name, strconv.Quote(rawPassword))
	}
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test path")
	}
	controlRoot := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
	for _, path := range []string{
		filepath.Join(controlRoot, "nats.conf"),
		filepath.Join(controlRoot, "..", "Model Plane", "deploy", "nats.conf"),
		filepath.Join(controlRoot, "..", "Application Plane", "nats.conf"),
	} {
		options, err := natsserver.ProcessConfigFile(path)
		if err != nil {
			t.Fatalf("parse scoped broker config %s: %v", path, err)
		}
		for _, user := range options.Users {
			if user.Password != rawPassword {
				t.Fatalf("broker config %s did not preserve the exact scoped password for %s", path, user.Username)
			}
		}
	}
}

func TestScopedBrokerMonitoringBindsLoopback(t *testing.T) {
	rawPassword := strconv.Quote("0123456789abcdef0123456789abcdef")
	for _, name := range []string{
		"AUTH_NATS_PASSWORD", "USER_NATS_PASSWORD", "ORG_NATS_PASSWORD", "BILLING_NATS_PASSWORD", "SESSION_NATS_PASSWORD",
		"AUDIT_CONTROL_NATS_PASSWORD", "CONTROL_NATS_PROVISIONER_PASSWORD",
		"AUTH_SHARED_NATS_PASSWORD", "USER_SHARED_NATS_PASSWORD", "ORG_SHARED_NATS_PASSWORD", "BILLING_SHARED_NATS_PASSWORD",
		"SESSION_SHARED_NATS_PASSWORD", "APPLICATION_CONVEX_CONTROL_NATS_PASSWORD", "CONTROL_SHARED_BRIDGE_PASSWORD",
		"DOCUMENTS_GDPR_NATS_PASSWORD",
		"CONTROL_SHARED_NATS_PROVISIONER_PASSWORD",
		"MODEL_NATS_RUNTIME_PASSWORD", "MODEL_GATEWAY_NATS_PASSWORD", "MODEL_SESSION_CORE_NATS_PASSWORD",
		"AUDIT_MODEL_NATS_PASSWORD", "MODEL_NATS_PROVISIONER_PASSWORD",
		"APPLICATION_CONVEX_MODEL_NATS_PASSWORD", "APPLICATION_INSIGHT_MODEL_NATS_PASSWORD",
		"APPLICATION_CONVERSATION_NATS_PASSWORD", "APPLICATION_SOCIAL_NATS_PASSWORD", "APPLICATION_INSIGHT_NATS_PASSWORD",
		"APPLICATION_LEADS_NATS_PASSWORD", "APPLICATION_NOTIFICATION_NATS_PASSWORD",
		"AUDIT_APPLICATION_NATS_PASSWORD", "APPLICATION_NATS_PROVISIONER_PASSWORD",
	} {
		t.Setenv(name, rawPassword)
	}
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test path")
	}
	controlRoot := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
	configs := map[string]string{
		"control":        filepath.Join(controlRoot, "nats.conf"),
		"control-shared": filepath.Join(controlRoot, "control-shared-nats.conf"),
		"model":          filepath.Join(controlRoot, "..", "Model Plane", "deploy", "nats.conf"),
		"application":    filepath.Join(controlRoot, "..", "Application Plane", "nats.conf"),
	}
	for name, path := range configs {
		options, err := natsserver.ProcessConfigFile(path)
		if err != nil {
			t.Fatalf("parse %s broker config: %v", name, err)
		}
		if options.HTTPHost != "127.0.0.1" || options.HTTPPort != 8222 {
			t.Errorf("%s monitoring listener = %s:%d, want 127.0.0.1:8222", name, options.HTTPHost, options.HTTPPort)
		}
	}
}

func TestControlReleaseUsesDistinctServicePrincipals(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test path")
	}
	controlRoot := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
	composeBytes, err := os.ReadFile(filepath.Join(controlRoot, "docker-compose.yml"))
	if err != nil {
		t.Fatal(err)
	}
	compose := string(composeBytes)
	brokerBytes, err := os.ReadFile(filepath.Join(controlRoot, "nats.conf"))
	if err != nil {
		t.Fatal(err)
	}
	broker := string(brokerBytes)

	principals := []struct {
		service  string
		user     string
		password string
	}{
		{service: "auth-core", user: "auth-core-control", password: "AUTH_NATS_PASSWORD"},
		{service: "user-core", user: "user-core-control", password: "USER_NATS_PASSWORD"},
		{service: "org-core", user: "org-core-control", password: "ORG_NATS_PASSWORD"},
		{service: "billing-core", user: "billing-core-control", password: "BILLING_NATS_PASSWORD"},
		{service: "session-core", user: "session-core-control", password: "SESSION_NATS_PASSWORD"},
	}
	seenPasswords := make(map[string]string, len(principals))
	for _, principal := range principals {
		block, blockErr := composeServiceBlock(compose, principal.service)
		if blockErr != nil {
			t.Fatal(blockErr)
		}
		if !strings.Contains(block, "NATS_USER: "+principal.user) ||
			!strings.Contains(block, "NATS_PASSWORD: ${"+principal.password+":?") {
			t.Fatalf("%s is not wired to its scoped principal", principal.service)
		}
		if strings.Contains(block, "\n      NATS_TOKEN:") {
			t.Fatalf("%s release block still wires legacy local NATS token auth", principal.service)
		}
		if !strings.Contains(broker, `user: "`+principal.user+`"`) ||
			!strings.Contains(broker, "password: $"+principal.password) {
			t.Fatalf("broker is missing scoped principal %s", principal.user)
		}
		if other, exists := seenPasswords[principal.password]; exists {
			t.Fatalf("%s reuses %s's broker password variable", principal.service, other)
		}
		seenPasswords[principal.password] = principal.service
	}
	if strings.Contains(compose, "CONTROL_NATS_RUNTIME_PASSWORD") || strings.Contains(broker, `user: "control-runtime"`) {
		t.Fatal("release configuration still contains the shared Control runtime principal")
	}
	for _, user := range []string{"auth-core-control", "user-core-control", "org-core-control", "billing-core-control", "session-core-control"} {
		block, blockErr := natsUserConfigBlock(broker, user)
		if blockErr != nil {
			t.Fatal(blockErr)
		}
		for _, forbidden := range []string{"$JS.API.STREAM.>", "$JS.API.CONSUMER.>", "$JS.ACK.>"} {
			if strings.Contains(block, forbidden) {
				t.Fatalf("runtime principal %s retains cross-service JetStream authority %q", user, forbidden)
			}
		}
		if strings.Contains(block, `"_INBOX.>"`) {
			t.Fatalf("runtime principal %s can subscribe to another service's replies", user)
		}
	}
	billingBlock, err := natsUserConfigBlock(broker, "billing-core-control")
	if err != nil {
		t.Fatal(err)
	}
	for _, required := range []string{
		"$JS.API.CONSUMER.INFO.CONTROL_PLANE_EVENTS.billing-core-organization-plan-changed",
		"$JS.ACK.CONTROL_PLANE_EVENTS.billing-core-organization-plan-changed.>",
		"_VELION.CONTROL.DELIVER.billing.organization-plan-changed billing-core-organization-plan-changed",
	} {
		if !strings.Contains(billingBlock, required) {
			t.Fatalf("billing principal missing fixed consumer capability %q", required)
		}
	}
}

func TestFreshControlBrokerSupportsScopedDomainAndRequestReplyContracts(t *testing.T) {
	password := "0123456789abcdef0123456789abcdef"
	for _, name := range []string{
		"AUTH_NATS_PASSWORD", "USER_NATS_PASSWORD", "ORG_NATS_PASSWORD",
		"BILLING_NATS_PASSWORD", "SESSION_NATS_PASSWORD",
		"AUDIT_CONTROL_NATS_PASSWORD", "CONTROL_NATS_PROVISIONER_PASSWORD",
	} {
		t.Setenv(name, strconv.Quote(password))
	}
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test path")
	}
	controlRoot := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
	options, err := natsserver.ProcessConfigFile(filepath.Join(controlRoot, "nats.conf"))
	if err != nil {
		t.Fatal(err)
	}
	options.Port = -1
	options.HTTPPort = -1
	options.StoreDir = t.TempDir()
	server, err := natsserver.NewServer(options)
	if err != nil {
		t.Fatal(err)
	}
	go server.Start()
	if !server.ReadyForConnections(10 * time.Second) {
		t.Fatal("fresh scoped Control broker did not start")
	}
	t.Cleanup(func() {
		server.Shutdown()
		server.WaitForShutdown()
	})

	connect := func(user, inbox string, extra ...nats.Option) *nats.Conn {
		opts := []nats.Option{nats.UserInfo(user, password), nats.CustomInboxPrefix(inbox)}
		opts = append(opts, extra...)
		connection, connectErr := nats.Connect(server.ClientURL(), opts...)
		if connectErr != nil {
			t.Fatalf("connect %s: %v", user, connectErr)
		}
		t.Cleanup(connection.Close)
		return connection
	}
	provisionerConnection := connect("observability-provisioner-control", "_INBOX.PROVISIONER_CONTROL")
	provisionerJS, err := provisionerConnection.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if err := provisioner.Provision(context.Background(), provisionerJS, provisioner.Bus{Name: "control", Plane: "control"}); err != nil {
		t.Fatalf("provision fresh Control broker: %v", err)
	}

	auth := connect("auth-core-control", "_INBOX.AUTH_CONTROL")
	userPermissionErrors := make(chan error, 4)
	user := connect("user-core-control", "_INBOX.USER_CONTROL", nats.ErrorHandler(func(_ *nats.Conn, _ *nats.Subscription, permissionErr error) {
		userPermissionErrors <- permissionErr
	}))
	org := connect("org-core-control", "_INBOX.ORG_CONTROL")
	billing := connect("billing-core-control", "_INBOX.BILLING_CONTROL")
	session := connect("session-core-control", "_INBOX.SESSION_CONTROL")

	validation, err := auth.Subscribe("session.validate", func(message *nats.Msg) {
		_ = message.Respond([]byte(`{"valid":true,"userId":"user-1"}`))
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = validation.Unsubscribe() })
	if err := auth.Flush(); err != nil {
		t.Fatal(err)
	}
	reply, err := session.Request("session.validate", []byte(`{"cookies":"fixture"}`), 2*time.Second)
	if err != nil || !strings.Contains(string(reply.Data), `"valid":true`) {
		t.Fatalf("scoped session validation request/reply failed: %s, %v", reply.Data, err)
	}

	for _, event := range []struct {
		connection *nats.Conn
		subject    string
	}{
		{auth, "auth.user.registered"},
		{user, "user.updated"},
		{org, "organization.updated"},
		{billing, "billing.account.updated"},
	} {
		js, jsErr := event.connection.JetStream()
		if jsErr != nil {
			t.Fatal(jsErr)
		}
		if _, publishErr := js.Publish(event.subject, []byte(`{"fixture":true}`)); publishErr != nil {
			t.Fatalf("fresh broker PubAck for %s: %v", event.subject, publishErr)
		}
	}

	billingJS, err := billing.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	delivered := make(chan struct{}, 1)
	planSubscription, err := billingJS.QueueSubscribe(
		provisioner.BillingPlanSubject,
		provisioner.BillingPlanConsumerName,
		func(message *nats.Msg) { _ = message.Ack(); delivered <- struct{}{} },
		nats.Bind(provisioner.ControlEventsStreamName, provisioner.BillingPlanConsumerName),
	)
	if err != nil {
		t.Fatalf("bind fixed billing consumer: %v", err)
	}
	t.Cleanup(func() { _ = planSubscription.Unsubscribe() })
	orgJS, err := org.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := orgJS.Publish(provisioner.BillingPlanSubject, []byte(`{"revision":1}`)); err != nil {
		t.Fatal(err)
	}
	select {
	case <-delivered:
	case <-time.After(3 * time.Second):
		t.Fatal("fixed billing consumer did not receive fresh-broker plan event")
	}

	_, _ = user.Subscribe("_INBOX.AUTH_CONTROL.spy", func(*nats.Msg) {})
	_ = user.Flush()
	select {
	case permissionErr := <-userPermissionErrors:
		if !strings.Contains(strings.ToLower(permissionErr.Error()), "permission") {
			t.Fatalf("unexpected cross-inbox error: %v", permissionErr)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("user principal could subscribe to Auth reply inbox")
	}
	userJS, err := user.JetStream(nats.MaxWait(time.Second))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := userJS.AddStream(&nats.StreamConfig{Name: "FORGED", Subjects: []string{"forged.>"}}); err == nil {
		t.Fatal("runtime user principal administered an unrelated stream")
	}
}

func natsUserConfigBlock(config, user string) (string, error) {
	marker := `user: "` + user + `"`
	start := strings.Index(config, marker)
	if start < 0 {
		return "", fmt.Errorf("NATS user %s not found", user)
	}
	rest := config[start+len(marker):]
	end := strings.Index(rest, "\n    {\n      user:")
	if end < 0 {
		end = strings.Index(rest, "\n  ]")
	}
	if end < 0 {
		return "", fmt.Errorf("NATS user %s block is unterminated", user)
	}
	return config[start : start+len(marker)+end], nil
}

func composeServiceBlock(compose, service string) (string, error) {
	startMarker := "  " + service + ":\n"
	start := -1
	if strings.HasPrefix(compose, startMarker) {
		start = 0
	} else if index := strings.Index(compose, "\n"+startMarker); index >= 0 {
		start = index + 1
	}
	if start < 0 {
		return "", fmt.Errorf("compose service %s not found", service)
	}
	restStart := start + len(startMarker)
	rest := compose[restStart:]
	for offset := 0; offset < len(rest); {
		next := strings.IndexByte(rest[offset:], '\n')
		if next < 0 {
			break
		}
		lineStart := offset + next + 1
		line := rest[lineStart:]
		if strings.HasPrefix(line, "  ") && !strings.HasPrefix(line, "   ") && !strings.HasPrefix(line, "  #") {
			return compose[start : restStart+lineStart-1], nil
		}
		offset = lineStart
	}
	return compose[start:], nil
}

func assertMainPermissionDenied(t *testing.T, connection *nats.Conn, permissionErrors <-chan error, operation string, action func() error) {
	t.Helper()
	if err := action(); err != nil && strings.Contains(strings.ToLower(err.Error()), "permission") {
		return
	}
	_ = connection.FlushTimeout(time.Second)
	select {
	case err := <-permissionErrors:
		if !strings.Contains(strings.ToLower(err.Error()), "permission") {
			t.Fatalf("%s returned unexpected asynchronous error: %v", operation, err)
		}
	case <-time.After(2 * time.Second):
		t.Fatalf("%s was not denied", operation)
	}
}

func TestScopedBrokerConfigsPinPlaneDeliverySubjects(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test path")
	}
	controlRoot := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
	configs := map[string]string{
		"control":     filepath.Join(controlRoot, "nats.conf"),
		"model":       filepath.Join(controlRoot, "..", "Model Plane", "deploy", "nats.conf"),
		"application": filepath.Join(controlRoot, "..", "Application Plane", "nats.conf"),
	}
	for plane, path := range configs {
		contents, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		text := string(contents)
		for _, kind := range []string{"audit", "usage"} {
			want := fmt.Sprintf("_VELION.AUDIT.DELIVER.%s.%s-v2 audit-core-%s-v3-%s", plane, kind, plane, kind)
			if !strings.Contains(text, want) {
				t.Fatalf("%s broker ACL missing %q", plane, want)
			}
		}
		for other := range configs {
			if other != plane && strings.Contains(text, "_VELION.AUDIT.DELIVER."+other+".") {
				t.Fatalf("%s broker ACL admits %s delivery subject", plane, other)
			}
		}
	}
}

func setMinimumConfig(t *testing.T) {
	t.Helper()
	t.Setenv("DATABASE_URL", "postgres://fixture.invalid/audit")
	t.Setenv("NATS_URL", "nats://controlplane-nats:4222")
	t.Setenv("NATS_USER", "audit-core-control")
	t.Setenv("NATS_PASSWORD", "0123456789abcdef0123456789abcdef")
	t.Setenv("NATS_TOKEN", "")
	t.Setenv("AUDIT_ALLOW_NATS_TOKEN_FALLBACK", "")
	t.Setenv("AUDIT_CORE_SERVICE_CREDENTIALS", serverTestCredentials)
	t.Setenv("EXTRA_NATS_URLS", "")
	t.Setenv("AUDIT_EXTRA_NATS_BUSES", "")
}

func TestLoadConfigRejectsUnsafePrimaryNATSBoundary(t *testing.T) {
	tests := []struct {
		name     string
		url      string
		user     string
		password string
	}{
		{name: "missing URL", user: "audit-core-control", password: "0123456789abcdef0123456789abcdef"},
		{name: "embedded credential", url: "nats://secret@controlplane-nats:4222", user: "audit-core-control", password: "0123456789abcdef0123456789abcdef"},
		{name: "missing user", url: "nats://controlplane-nats:4222", password: "0123456789abcdef0123456789abcdef"},
		{name: "missing password", url: "nats://controlplane-nats:4222", user: "audit-core-control"},
		{name: "short password", url: "nats://controlplane-nats:4222", user: "audit-core-control", password: "short"},
		{name: "placeholder password", url: "nats://controlplane-nats:4222", user: "audit-core-control", password: "placeholder-primary-token-0123456789"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			setMinimumConfig(t)
			t.Setenv("NATS_URL", test.url)
			t.Setenv("NATS_USER", test.user)
			t.Setenv("NATS_PASSWORD", test.password)
			if _, err := loadConfig(); err == nil {
				t.Fatal("unsafe primary NATS boundary was accepted")
			}
		})
	}
}

func TestLoadConfigRequiresExplicitMigrationSwitchForTokenFallback(t *testing.T) {
	setMinimumConfig(t)
	t.Setenv("NATS_USER", "")
	t.Setenv("NATS_PASSWORD", "")
	t.Setenv("NATS_TOKEN", "0123456789abcdef0123456789abcdef")
	if _, err := loadConfig(); err == nil {
		t.Fatal("implicit token fallback was accepted")
	}
	t.Setenv("AUDIT_ALLOW_NATS_TOKEN_FALLBACK", "1")
	cfg, err := loadConfig()
	if err != nil {
		t.Fatalf("explicit migration fallback rejected: %v", err)
	}
	if cfg.NATSUser != "" || cfg.NATSToken == "" {
		t.Fatalf("unexpected fallback config: %+v", cfg)
	}
}

func TestLoadConfigPrefersScopedUserPasswordOverMigrationToken(t *testing.T) {
	setMinimumConfig(t)
	t.Setenv("NATS_TOKEN", "abcdef0123456789abcdef0123456789")
	t.Setenv("AUDIT_ALLOW_NATS_TOKEN_FALLBACK", "1")
	cfg, err := loadConfig()
	if err != nil {
		t.Fatal(err)
	}
	if cfg.NATSUser != "audit-core-control" || cfg.NATSPassword == "" || cfg.NATSToken != "" {
		t.Fatalf("scoped credentials were not preferred: %+v", cfg)
	}
}

func TestLoadConfigParsesStableCredentialedExtraBuses(t *testing.T) {
	setMinimumConfig(t)
	t.Setenv("AUDIT_EXTRA_NATS_BUSES", `[{"name":"model","plane":"model","url":"nats://model-nats:4222","user":"audit-core-model","password":"abcdef0123456789abcdef0123456789"},{"name":"application","plane":"application","url":"nats://application-nats:4222","user":"audit-core-application","password":"fedcba9876543210fedcba9876543210"}]`)
	cfg, err := loadConfig()
	if err != nil {
		t.Fatal(err)
	}
	if len(cfg.ExtraNATSBuses) != 2 || cfg.ExtraNATSBuses[0].Plane != "model" || cfg.ExtraNATSBuses[1].Plane != "application" {
		t.Fatalf("extra buses = %+v", cfg.ExtraNATSBuses)
	}
	if cfg.ServiceCredentials != serverTestCredentials {
		t.Fatal("service credential registry was not retained")
	}
}

func TestLoadConfigRejectsAmbiguousOrUnsafeExtraBuses(t *testing.T) {
	tests := []struct {
		name   string
		legacy string
		raw    string
	}{
		{name: "legacy positional URLs", legacy: "nats://legacy:4222"},
		{name: "malformed JSON", raw: `not-json`},
		{name: "missing credential", raw: `[{"name":"model","plane":"model","url":"nats://model:4222"}]`},
		{name: "missing plane authority", raw: `[{"name":"model","url":"nats://model:4222","user":"audit-core-model","password":"0123456789abcdef0123456789abcdef"}]`},
		{name: "embedded credential", raw: `[{"name":"model","plane":"model","url":"nats://secret@model:4222","user":"audit-core-model","password":"0123456789abcdef0123456789abcdef"}]`},
		{name: "duplicate stable name", raw: `[{"name":"model","plane":"model","url":"nats://one:4222","user":"audit-core-model","password":"0123456789abcdef0123456789abcdef"},{"name":"model","plane":"model","url":"nats://two:4222","user":"audit-core-model-two","password":"abcdef0123456789abcdef0123456789"}]`},
		{name: "reused plane credential", raw: `[{"name":"model","plane":"model","url":"nats://one:4222","user":"audit-core-model","password":"0123456789abcdef0123456789abcdef"},{"name":"application","plane":"application","url":"nats://two:4222","user":"audit-core-application","password":"0123456789abcdef0123456789abcdef"}]`},
		{name: "invalid name", raw: `[{"name":"../model","plane":"model","url":"nats://model:4222","user":"audit-core-model","password":"0123456789abcdef0123456789abcdef"}]`},
		{name: "jetstream token separator", raw: `[{"name":"model.prod","plane":"model","url":"nats://model:4222","user":"audit-core-model","password":"0123456789abcdef0123456789abcdef"}]`},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			setMinimumConfig(t)
			t.Setenv("EXTRA_NATS_URLS", test.legacy)
			t.Setenv("AUDIT_EXTRA_NATS_BUSES", test.raw)
			if _, err := loadConfig(); err == nil {
				t.Fatal("unsafe extra bus configuration was accepted")
			}
		})
	}
}

func TestValidateNATSURLAllowsOnlyCredentialFreeBrokerURLs(t *testing.T) {
	for _, raw := range []string{
		"nats://broker:4222", "nats://broker:4222/", "tls://broker:4222", "ws://broker:8080", "wss://broker.example",
	} {
		if err := validateNATSURL(raw); err != nil {
			t.Errorf("valid URL %q rejected: %v", raw, err)
		}
	}
	for _, raw := range []string{
		"%", "nats://", "http://broker:4222", "nats://user:pass@broker:4222",
		"nats://broker:4222/path", "nats://broker:4222?token=secret", "nats://broker:4222#fragment",
	} {
		if err := validateNATSURL(raw); err == nil {
			t.Errorf("unsafe URL %q accepted", raw)
		}
	}
}

func TestConnectNamedNATSUsesScopedUserPassword(t *testing.T) {
	const password = "0123456789abcdef0123456789abcdef"
	natsServer, err := natsserver.NewServer(&natsserver.Options{
		JetStream: true, StoreDir: t.TempDir(), Port: -1, Username: "audit-core-model", Password: password,
	})
	if err != nil {
		t.Fatal(err)
	}
	go natsServer.Start()
	if !natsServer.ReadyForConnections(10 * time.Second) {
		t.Fatal("NATS did not become ready")
	}
	t.Cleanup(func() { natsServer.Shutdown(); natsServer.WaitForShutdown() })
	connection, err := connectNamedNATS(extraNATSBus{
		Name: "model", Plane: "model", URL: natsServer.ClientURL(), User: "audit-core-model", Password: password,
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(connection.Close)
	if !connection.IsConnected() {
		t.Fatal("scoped user/password connection is not connected")
	}
}

type natsRecordingStore struct {
	audits chan *events.AuditEvent
}

func (s *natsRecordingStore) InsertAuditFromStream(_ context.Context, event *events.AuditEvent, _, _ string, _ uint64) (bool, error) {
	s.audits <- event
	return true, nil
}

func (s *natsRecordingStore) InsertUsageFromStream(context.Context, *events.UsageEvent, string, string, uint64) (bool, error) {
	return true, nil
}

func TestCredentialedNamedBusConsumesAndReconnectsWithStableIdentity(t *testing.T) {
	storeDir := t.TempDir()
	token := "0123456789abcdef0123456789abcdef"
	first := startTokenNATSServer(t, storeDir, -1, token)
	port := first.Addr().(*net.TCPAddr).Port
	bus := extraNATSBus{Name: "model", Plane: "model", URL: first.ClientURL(), Token: token}
	nc, err := connectNamedNATS(bus)
	if err != nil {
		t.Fatalf("connect named bus: %v", err)
	}
	t.Cleanup(nc.Close)
	provisionMainTestObservability(t, nc, bus.Name, bus.Plane)
	st := &natsRecordingStore{audits: make(chan *events.AuditEvent, 2)}
	sub := subscriber.New(nc, st, bus.Name)
	if err := sub.Start(context.Background()); err != nil {
		t.Fatalf("start subscriber: %v", err)
	}

	publishAuditEvent(t, nc, "before-reconnect")
	waitAuditEvent(t, st.audits, "before-reconnect")
	first.Shutdown()
	first.WaitForShutdown()

	second := startTokenNATSServer(t, storeDir, port, token)
	t.Cleanup(func() {
		second.Shutdown()
		second.WaitForShutdown()
	})
	deadline := time.Now().Add(10 * time.Second)
	for !nc.IsConnected() && time.Now().Before(deadline) {
		time.Sleep(10 * time.Millisecond)
	}
	if !nc.IsConnected() {
		t.Fatal("named bus did not reconnect")
	}
	publishAuditEvent(t, nc, "after-reconnect")
	waitAuditEvent(t, st.audits, "after-reconnect")

	health := sub.Health()
	if health.Bus != "model" || health.Audit.Consumer != "audit-core-model-v3-audit" || health.Usage.Consumer != "audit-core-model-v3-usage" {
		t.Fatalf("unstable subscriber health identity: %+v", health)
	}
}

func TestManagedNamedBusRecoversWhenInitiallyUnavailable(t *testing.T) {
	token := "0123456789abcdef0123456789abcdef"
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	port := listener.Addr().(*net.TCPAddr).Port
	if err := listener.Close(); err != nil {
		t.Fatal(err)
	}

	ctx, cancel := context.WithCancel(context.Background())
	t.Cleanup(cancel)
	managed := startManagedExtraNATSBus(ctx, extraNATSBus{
		Name:  "ingestion",
		Plane: "ingestion",
		URL:   "nats://127.0.0.1:" + fmt.Sprint(port),
		Token: token,
	}, store.New(nil))
	t.Cleanup(managed.Close)

	if readiness := managed.Readiness(); readiness.Ready() {
		t.Fatalf("initially unavailable bus reported ready: %+v", readiness)
	}

	natsServer := startTokenNATSServer(t, t.TempDir(), port, token)
	t.Cleanup(func() {
		natsServer.Shutdown()
		natsServer.WaitForShutdown()
	})
	provisioner, err := nats.Connect(natsServer.ClientURL(), nats.Token(token))
	if err != nil {
		t.Fatal(err)
	}
	provisionMainTestObservability(t, provisioner, "ingestion", "ingestion")
	provisioner.Close()
	deadline := time.Now().Add(10 * time.Second)
	for time.Now().Before(deadline) {
		readiness := managed.Readiness()
		if readiness.Ready() {
			if readiness.Name != "ingestion" ||
				readiness.Audit.Consumer != "audit-core-ingestion-v3-audit" ||
				readiness.Usage.Consumer != "audit-core-ingestion-v3-usage" {
				t.Fatalf("unstable managed bus identity: %+v", readiness)
			}
			return
		}
		time.Sleep(20 * time.Millisecond)
	}
	t.Fatalf("managed bus did not become ready after server recovery: %+v", managed.Readiness())
}

func TestManagedNamedBusClearsReadyStateAfterTerminalConnectionClose(t *testing.T) {
	token := "0123456789abcdef0123456789abcdef"
	natsServer := startTokenNATSServer(t, t.TempDir(), -1, token)
	url := natsServer.ClientURL()
	connection, err := connectNamedNATS(extraNATSBus{
		Name:  "model",
		Plane: "model",
		URL:   url,
		Token: token,
	})
	if err != nil {
		t.Fatalf("connect named bus: %v", err)
	}
	connection.Close()
	natsServer.Shutdown()
	natsServer.WaitForShutdown()

	managed := &managedExtraNATSBus{
		config:          extraNATSBus{Name: "model", Plane: "model", URL: url, Token: token},
		store:           store.New(nil),
		connection:      connection,
		subscriberReady: true,
	}
	managed.ensureStarted(context.Background())

	managed.mu.RLock()
	defer managed.mu.RUnlock()
	if managed.subscriberReady || (managed.connection != nil && managed.connection.IsConnected()) {
		t.Fatalf("terminal connection retained stale ready state: ready=%v connection=%v", managed.subscriberReady, managed.connection)
	}
}

func TestManagedNamedBusClearsConnectionWhenSubscriberResourcesAreMissing(t *testing.T) {
	token := "0123456789abcdef0123456789abcdef"
	natsServer := startTokenNATSServer(t, t.TempDir(), -1, token)
	t.Cleanup(func() { natsServer.Shutdown(); natsServer.WaitForShutdown() })
	managed := &managedExtraNATSBus{
		config: extraNATSBus{Name: "model", Plane: "model", URL: natsServer.ClientURL(), Token: token},
		store:  store.New(nil),
	}
	managed.ensureStarted(context.Background())
	managed.mu.RLock()
	connection := managed.connection
	busSubscriber := managed.subscriber
	ready := managed.subscriberReady
	managed.mu.RUnlock()
	if connection != nil || busSubscriber != nil || ready {
		t.Fatalf("failed subscriber retained runtime state: connection=%v subscriber=%v ready=%v", connection, busSubscriber, ready)
	}
}

func TestManagedNamedBusDoesNotRestartHealthySubscriber(t *testing.T) {
	token := "0123456789abcdef0123456789abcdef"
	natsServer := startTokenNATSServer(t, t.TempDir(), -1, token)
	t.Cleanup(func() { natsServer.Shutdown(); natsServer.WaitForShutdown() })
	connection, err := nats.Connect(natsServer.ClientURL(), nats.Token(token))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(connection.Close)
	managed := &managedExtraNATSBus{
		config: extraNATSBus{Name: "model", Plane: "model", URL: natsServer.ClientURL(), Token: token},
		store:  store.New(nil), connection: connection, subscriberReady: true,
	}
	managed.ensureStarted(context.Background())
	managed.mu.RLock()
	defer managed.mu.RUnlock()
	if managed.connection != connection || !managed.subscriberReady {
		t.Fatal("healthy subscriber was restarted")
	}
}

func startTokenNATSServer(t *testing.T, storeDir string, port int, token string) *natsserver.Server {
	t.Helper()
	server, err := natsserver.NewServer(&natsserver.Options{
		JetStream:     true,
		StoreDir:      storeDir,
		Port:          port,
		Authorization: token,
	})
	if err != nil {
		t.Fatal(err)
	}
	go server.Start()
	if !server.ReadyForConnections(10 * time.Second) {
		t.Fatal("NATS server did not become ready")
	}
	return server
}

func publishAuditEvent(t *testing.T, nc *nats.Conn, eventName string) {
	t.Helper()
	payload, err := json.Marshal(events.AuditEvent{
		EventID: "audit-model-gateway-" + eventName, OccurredAt: time.Now().UTC(), OrgID: "org-fixture",
		Plane: "model", Producer: "model-gateway", Event: eventName,
	})
	if err != nil {
		t.Fatal(err)
	}
	js, err := nc.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := js.Publish("velion.audit.v2.model.model-gateway."+eventName, payload); err != nil {
		t.Fatal(err)
	}
}

func provisionMainTestObservability(t *testing.T, nc *nats.Conn, bus, plane string) {
	t.Helper()
	js, err := nc.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := js.StreamInfo("VELION_CONTROL_OBSERVABILITY"); errors.Is(err, nats.ErrStreamNotFound) {
		if _, err := js.AddStream(&nats.StreamConfig{
			Name:       "VELION_CONTROL_OBSERVABILITY",
			Subjects:   []string{"velion.audit.v2." + plane + ".>", "velion.usage.v2." + plane + ".>", "velion.dlq.audit-core.>"},
			Retention:  nats.LimitsPolicy,
			Storage:    nats.FileStorage,
			Discard:    nats.DiscardOld,
			MaxAge:     30 * 24 * time.Hour,
			Duplicates: 2 * time.Minute,
		}); err != nil {
			t.Fatal(err)
		}
	} else if err != nil {
		t.Fatal(err)
	}
	for _, kind := range []string{"audit", "usage"} {
		consumer := fmt.Sprintf("audit-core-%s-v3-%s", bus, kind)
		if _, err := js.ConsumerInfo("VELION_CONTROL_OBSERVABILITY", consumer); err == nil {
			continue
		} else if !errors.Is(err, nats.ErrConsumerNotFound) {
			t.Fatal(err)
		}
		if _, err := js.AddConsumer("VELION_CONTROL_OBSERVABILITY", &nats.ConsumerConfig{
			Durable:        consumer,
			DeliverSubject: fmt.Sprintf("_VELION.AUDIT.DELIVER.%s.%s-v2", plane, kind),
			DeliverGroup:   consumer,
			FilterSubject:  fmt.Sprintf("velion.%s.v2.%s.>", kind, plane),
			DeliverPolicy:  nats.DeliverAllPolicy,
			AckPolicy:      nats.AckExplicitPolicy,
			AckWait:        30 * time.Second,
			MaxDeliver:     5,
			ReplayPolicy:   nats.ReplayInstantPolicy,
		}); err != nil {
			t.Fatal(err)
		}
	}
}

func waitAuditEvent(t *testing.T, events <-chan *events.AuditEvent, eventName string) {
	t.Helper()
	select {
	case event := <-events:
		if event.Event != eventName {
			t.Fatalf("event = %q; want %q", event.Event, eventName)
		}
	case <-time.After(10 * time.Second):
		t.Fatalf("timed out waiting for %s", eventName)
	}
}
