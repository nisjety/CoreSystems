package main

import (
	"context"
	"strings"
	"testing"
	"time"

	server "github.com/nats-io/nats-server/v2/server"
	"github.com/nats-io/nats.go"
	"github.com/triodelab/controlplane/audit-core/internal/provisioner"
)

func TestBridgeForwardsProvisionedControlEventsAndAcknowledges(t *testing.T) {
	start := func(jetStream bool) *server.Server {
		instance, err := server.NewServer(&server.Options{JetStream: jetStream, StoreDir: t.TempDir(), Port: -1})
		if err != nil {
			t.Fatal(err)
		}
		go instance.Start()
		if !instance.ReadyForConnections(10 * time.Second) {
			t.Fatal("NATS server did not become ready")
		}
		t.Cleanup(func() { instance.Shutdown(); instance.WaitForShutdown() })
		return instance
	}
	sourceServer := start(true)
	legacyServer := start(true)
	source, err := nats.Connect(sourceServer.ClientURL())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(source.Close)
	legacy, err := nats.Connect(legacyServer.ClientURL())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(legacy.Close)
	js, err := source.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if err := provisioner.ProvisionControlSharedRuntime(context.Background(), js); err != nil {
		t.Fatal(err)
	}
	legacyJS, err := legacy.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := legacyJS.AddStream(&nats.StreamConfig{
		Name: "LEGACY_CONTROL", Subjects: []string{"aqencia.controlplane.>", "velion.gdpr.>"},
		Storage: nats.FileStorage, Duplicates: 2 * time.Minute,
	}); err != nil {
		t.Fatal(err)
	}
	received := make(chan *nats.Msg, 3)
	if _, err := legacy.Subscribe("aqencia.controlplane.org.*", func(message *nats.Msg) {
		received <- &nats.Msg{Subject: message.Subject, Header: message.Header, Data: append([]byte(nil), message.Data...)}
	}); err != nil {
		t.Fatal(err)
	}
	if err := legacy.Flush(); err != nil {
		t.Fatal(err)
	}
	notificationReceived := make(chan *nats.Msg, 1)
	if _, err := legacy.Subscribe("notifications.org.member_removed", func(message *nats.Msg) {
		notificationReceived <- message
	}); err != nil {
		t.Fatal(err)
	}
	if err := legacy.Flush(); err != nil {
		t.Fatal(err)
	}
	bridge, err := startBridge(source, legacy)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = bridge.Unsubscribe() })
	if _, err := js.Publish("aqencia.controlplane.org.created", []byte(`{"org_id":"org-1"}`), nats.MsgId("organization:org-1:1:created")); err != nil {
		t.Fatal(err)
	}
	if _, err := js.Publish("aqencia.controlplane.org.updated", []byte(`{"org_id":"org-1"}`)); err != nil {
		t.Fatal(err)
	}
	if _, err := js.Publish("notifications.org.member_removed", []byte(`{"org_id":"org-1"}`)); err != nil {
		t.Fatal(err)
	}
	select {
	case <-notificationReceived:
	case <-time.After(3 * time.Second):
		t.Fatal("legacy core-NATS notification was not bridged")
	}
	seen := map[string]*nats.Msg{}
	for len(seen) < 2 {
		select {
		case message := <-received:
			seen[message.Subject] = message
		case <-time.After(3 * time.Second):
			t.Fatalf("legacy subscriber received %d of 2 bridged events", len(seen))
		}
	}
	message := seen["aqencia.controlplane.org.created"]
	if message == nil {
		t.Fatal("legacy subscriber did not receive created event")
	}
	if string(message.Data) != `{"org_id":"org-1"}` {
		t.Fatalf("payload = %s", message.Data)
	}
	if message.Header.Get(nats.MsgIdHdr) != "organization:org-1:1:created" {
		t.Fatalf("bridged Msg-Id = %q", message.Header.Get(nats.MsgIdHdr))
	}
	derived := seen["aqencia.controlplane.org.updated"].Header.Get(nats.MsgIdHdr)
	if !strings.HasPrefix(derived, "control-shared:"+provisioner.ControlSharedStreamName+":") {
		t.Fatalf("derived bridged Msg-Id = %q", derived)
	}

	// A bridge crash after target PubAck but before source ACK redelivers the
	// same source identity. The legacy stream must de-duplicate it.
	if err := forwardToLegacy(legacy, legacyJS, &nats.Msg{
		Subject: "aqencia.controlplane.org.created",
		Header:  nats.Header{nats.MsgIdHdr: []string{"organization:org-1:1:created"}},
		Data:    []byte(`{"org_id":"org-1"}`),
	}); err != nil {
		t.Fatal(err)
	}
	info, err := legacyJS.StreamInfo("LEGACY_CONTROL")
	if err != nil {
		t.Fatal(err)
	}
	if info.State.Msgs != 2 {
		t.Fatalf("legacy stream stored %d events; want 2 after duplicate retry", info.State.Msgs)
	}
	if err := forwardToLegacy(legacy, legacyJS, &nats.Msg{
		Subject: provisioner.GDPRErasureRequestedSubject,
		Header:  nats.Header{nats.MsgIdHdr: []string{"gdpr:fanout:scoped-only"}},
		Data:    []byte(`{"subject_id":"user-sensitive","org_id":"org-1"}`),
	}); err != nil {
		t.Fatal(err)
	}
	info, err = legacyJS.StreamInfo("LEGACY_CONTROL")
	if err != nil {
		t.Fatal(err)
	}
	if info.State.Msgs != 2 {
		t.Fatalf("scoped-only GDPR evidence leaked to token broker; messages=%d", info.State.Msgs)
	}
}
