package subscriber

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	server "github.com/nats-io/nats-server/v2/server"
	"github.com/nats-io/nats.go"

	"github.com/triodelab/controlplane/audit-core/internal/events"
)

const (
	scopedACLPlane       = "model"
	scopedACLBus         = "model"
	scopedACLAuditUser   = "audit-core-model"
	scopedACLAuditPass   = "0123456789abcdef0123456789abcdef"
	scopedACLAdminUser   = "observability-provisioner"
	scopedACLAdminPass   = "abcdef0123456789abcdef0123456789"
	scopedACLInboxPrefix = "_INBOX.AUDIT_MODEL"
)

func TestScopedAuditPrincipalCanConsumeAndDeadLetterButCannotAdministerBroker(t *testing.T) {
	natsServer := startScopedACLServer(t)
	admin := connectScopedUser(t, natsServer.ClientURL(), scopedACLAdminUser, scopedACLAdminPass)
	provisionScopedObservability(t, admin, scopedACLBus, scopedACLPlane)

	permissionErrors := make(chan error, 16)
	audit, err := nats.Connect(
		natsServer.ClientURL(),
		nats.UserInfo(scopedACLAuditUser, scopedACLAuditPass),
		nats.CustomInboxPrefix(scopedACLInboxPrefix),
		nats.ErrorHandler(func(_ *nats.Conn, _ *nats.Subscription, permissionErr error) {
			permissionErrors <- permissionErr
		}),
	)
	if err != nil {
		t.Fatalf("connect scoped audit principal: %v", err)
	}
	t.Cleanup(audit.Close)

	st := &recordingStore{
		audits: make(chan *events.AuditEvent, 2),
		usage:  make(chan *events.UsageEvent, 2),
	}
	sub := New(audit, st, scopedACLBus, scopedACLPlane)
	if err := sub.Start(context.Background()); err != nil {
		t.Fatalf("bind pre-provisioned consumers: %v", err)
	}

	payload, err := json.Marshal(events.AuditEvent{
		EventID:    "audit-session-core-allowed-1",
		OccurredAt: time.Now().UTC(),
		OrgID:      "org-scoped-acl-fixture",
		Plane:      scopedACLPlane,
		Producer:   "session-core",
		Event:      "allowed",
	})
	if err != nil {
		t.Fatal(err)
	}
	adminJS, err := admin.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := adminJS.Publish("velion.audit.v2.model.session-core.allowed", payload); err != nil {
		t.Fatalf("publish allowed event: %v", err)
	}
	select {
	case event := <-st.audits:
		if event.Event != "allowed" {
			t.Fatalf("event = %q", event.Event)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("scoped audit principal did not ACK an allowed event")
	}

	if _, err := adminJS.Publish("velion.audit.v2.model.session-core.malformed", []byte("not-json")); err != nil {
		t.Fatalf("publish malformed event: %v", err)
	}
	waitForDeadLetter(t, adminJS, "audit", "malformed", "not-json")
	if health := sub.Health(); !health.Audit.Ready || !health.Usage.Ready {
		t.Fatalf("scoped consumer health is not ready: %+v", health)
	}

	assertPermissionDenied(t, audit, permissionErrors, "arbitrary subscribe", func() error {
		_, subscribeErr := audit.Subscribe(">", func(*nats.Msg) {})
		return subscribeErr
	})
	assertPermissionDenied(t, audit, permissionErrors, "arbitrary publish", func() error {
		return audit.Publish("service.authenticate", []byte(`{"forged":true}`))
	})
	assertPermissionDenied(t, audit, permissionErrors, "stream administration", func() error {
		return audit.Publish("$JS.API.STREAM.UPDATE.UNRELATED", []byte(`{}`))
	})
	assertPermissionDenied(t, audit, permissionErrors, "consumer administration", func() error {
		return audit.Publish("$JS.API.CONSUMER.CREATE.VELION_CONTROL_OBSERVABILITY.forged.velion.audit.v2.model.>", []byte(`{}`))
	})
}

func startScopedACLServer(t *testing.T) *server.Server {
	t.Helper()
	configPath := filepath.Join(t.TempDir(), "nats.conf")
	config := fmt.Sprintf(`
authorization {
  users = [
    {user: %q, password: %q, permissions: {publish: ">", subscribe: ">"}}
    {
      user: %q
      password: %q
      permissions: {
        publish: {allow: [
          "$JS.API.CONSUMER.INFO.VELION_CONTROL_OBSERVABILITY.audit-core-model-v3-audit"
          "$JS.API.CONSUMER.INFO.VELION_CONTROL_OBSERVABILITY.audit-core-model-v3-usage"
          "$JS.ACK.VELION_CONTROL_OBSERVABILITY.audit-core-model-v3-audit.>"
          "$JS.ACK.VELION_CONTROL_OBSERVABILITY.audit-core-model-v3-usage.>"
          "velion.dlq.audit-core.audit"
          "velion.dlq.audit-core.usage"
        ]}
        subscribe: {allow: [
          "%s.>"
          "_VELION.AUDIT.DELIVER.model.audit-v2 audit-core-model-v3-audit"
          "_VELION.AUDIT.DELIVER.model.usage-v2 audit-core-model-v3-usage"
        ]}
      }
    }
  ]
}
`, scopedACLAdminUser, scopedACLAdminPass, scopedACLAuditUser, scopedACLAuditPass, scopedACLInboxPrefix)
	if err := os.WriteFile(configPath, []byte(config), 0o600); err != nil {
		t.Fatal(err)
	}
	opts, err := server.ProcessConfigFile(configPath)
	if err != nil {
		t.Fatalf("parse scoped NATS config: %v", err)
	}
	opts.Port = -1
	opts.JetStream = true
	opts.StoreDir = t.TempDir()
	natsServer, err := server.NewServer(opts)
	if err != nil {
		t.Fatalf("create scoped NATS server: %v", err)
	}
	go natsServer.Start()
	if !natsServer.ReadyForConnections(10 * time.Second) {
		t.Fatal("scoped NATS server did not become ready")
	}
	t.Cleanup(func() {
		natsServer.Shutdown()
		natsServer.WaitForShutdown()
	})
	return natsServer
}

func connectScopedUser(t *testing.T, url, user, password string) *nats.Conn {
	t.Helper()
	connection, err := nats.Connect(url, nats.UserInfo(user, password))
	if err != nil {
		t.Fatalf("connect %s: %v", user, err)
	}
	t.Cleanup(connection.Close)
	return connection
}

func provisionScopedObservability(t *testing.T, connection *nats.Conn, bus, plane string) {
	t.Helper()
	js, err := connection.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := js.AddStream(&nats.StreamConfig{
		Name:       streamName,
		Subjects:   []string{planeSubject("audit", plane), planeSubject("usage", plane), dlqSubject},
		Retention:  nats.LimitsPolicy,
		Storage:    nats.FileStorage,
		Discard:    nats.DiscardOld,
		MaxAge:     30 * 24 * time.Hour,
		Duplicates: 2 * time.Minute,
	}); err != nil {
		t.Fatalf("provision stream: %v", err)
	}
	for _, kind := range []string{"audit", "usage"} {
		consumer := fmt.Sprintf("audit-core-%s-v3-%s", bus, kind)
		if _, err := js.AddConsumer(streamName, &nats.ConsumerConfig{
			Durable:        consumer,
			DeliverSubject: fmt.Sprintf("_VELION.AUDIT.DELIVER.%s.%s-v2", plane, kind),
			DeliverGroup:   consumer,
			FilterSubject:  planeSubject(kind, plane),
			DeliverPolicy:  nats.DeliverAllPolicy,
			AckPolicy:      nats.AckExplicitPolicy,
			AckWait:        30 * time.Second,
			MaxDeliver:     maxConsumerDeliveries,
			ReplayPolicy:   nats.ReplayInstantPolicy,
		}); err != nil {
			t.Fatalf("provision %s consumer: %v", kind, err)
		}
	}
}

func assertPermissionDenied(t *testing.T, connection *nats.Conn, errors <-chan error, operation string, action func() error) {
	t.Helper()
	if err := action(); err != nil && strings.Contains(strings.ToLower(err.Error()), "permission") {
		return
	}
	_ = connection.FlushTimeout(time.Second)
	select {
	case err := <-errors:
		if !strings.Contains(strings.ToLower(err.Error()), "permission") {
			t.Fatalf("%s returned unexpected asynchronous error: %v", operation, err)
		}
	case <-time.After(2 * time.Second):
		t.Fatalf("%s was not denied", operation)
	}
}
