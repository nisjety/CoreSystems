package provisioner

import (
	"context"
	server "github.com/nats-io/nats-server/v2/server"
	"github.com/nats-io/nats.go"
	"testing"
	"time"
)

func legacyNamespaceFixture(t *testing.T) nats.JetStreamContext {
	t.Helper()
	srv, err := server.NewServer(&server.Options{JetStream: true, StoreDir: t.TempDir(), Port: -1})
	if err != nil {
		t.Fatal(err)
	}
	go srv.Start()
	if !srv.ReadyForConnections(5 * time.Second) {
		t.Fatal("broker did not start")
	}
	t.Cleanup(func() { srv.Shutdown(); srv.WaitForShutdown() })
	nc, err := nats.Connect(srv.ClientURL())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(nc.Close)
	js, err := nc.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	_, err = js.AddStream(&nats.StreamConfig{Name: legacyControlSharedStream, Subjects: []string{"aqencia.controlplane.>", GDPRErasureRequestedSubject}, Storage: nats.FileStorage})
	if err != nil {
		t.Fatal(err)
	}
	_, err = js.AddConsumer(legacyControlSharedStream, &nats.ConsumerConfig{Durable: "prior-consumer", AckPolicy: nats.AckExplicitPolicy, FilterSubject: "aqencia.controlplane.>"})
	if err != nil {
		t.Fatal(err)
	}
	return js
}
func TestNamespaceCutoverPreservesHistoryAndConsumerPositions(t *testing.T) {
	js := legacyNamespaceFixture(t)
	ack, err := js.Publish("aqencia.controlplane.org.changed", []byte("historical-event"))
	if err != nil {
		t.Fatal(err)
	}
	sub, err := js.PullSubscribe("aqencia.controlplane.>", "prior-consumer", nats.Bind(legacyControlSharedStream, "prior-consumer"))
	if err != nil {
		t.Fatal(err)
	}
	messages, err := sub.Fetch(1, nats.MaxWait(time.Second))
	if err != nil {
		t.Fatal(err)
	}
	if err := messages[0].AckSync(); err != nil {
		t.Fatal(err)
	}
	sub.Unsubscribe()
	before, _ := js.ConsumerInfo(legacyControlSharedStream, "prior-consumer")
	saved := false
	save := func(snapshot NamespaceSnapshot) error {
		if snapshot.Stream.State.Msgs != 1 || len(snapshot.Consumers) != 1 {
			t.Fatal("snapshot lost history or consumer state")
		}
		current, _ := js.StreamInfo(legacyControlSharedStream)
		if namespaceIsArchived(current) {
			t.Fatal("snapshot after mutation")
		}
		saved = true
		return nil
	}
	if err := MigrateControlSharedNamespace(context.Background(), js, true, true, save); err != nil {
		t.Fatal(err)
	}
	if !saved {
		t.Fatal("missing snapshot")
	}
	old, err := js.GetMsg(legacyControlSharedStream, ack.Sequence)
	if err != nil || string(old.Data) != "historical-event" {
		t.Fatal("history lost", err)
	}
	after, _ := js.ConsumerInfo(legacyControlSharedStream, "prior-consumer")
	if before.AckFloor.Consumer != after.AckFloor.Consumer || before.AckFloor.Stream != after.AckFloor.Stream {
		t.Fatal("durable position changed")
	}
	if _, err := js.Publish("controlplane.org.changed", []byte("current-event")); err != nil {
		t.Fatal(err)
	}
	if err := MigrateControlSharedNamespace(context.Background(), js, true, true, nil); err != nil {
		t.Fatal("retry is not idempotent", err)
	}
}
func TestNamespaceMigrationRefusesPendingConsumersAndNormalProvisioning(t *testing.T) {
	js := legacyNamespaceFixture(t)
	if _, err := js.Publish("aqencia.controlplane.user.deleted", []byte("must-be-delivered")); err != nil {
		t.Fatal(err)
	}
	if err := ProvisionControlSharedRuntime(context.Background(), js); err == nil {
		t.Fatal("normal provisioning bypassed migration")
	}
	if err := MigrateControlSharedNamespace(context.Background(), js, true, true, func(NamespaceSnapshot) error { return nil }); err == nil {
		t.Fatal("discarded pending work")
	}
	old, _ := js.StreamInfo(legacyControlSharedStream)
	if namespaceIsArchived(old) || old.State.Msgs != 1 {
		t.Fatal("refusal mutated stream")
	}
	if _, err := js.StreamInfo(ControlSharedStreamName); err == nil {
		t.Fatal("refusal created target")
	}
}
func TestNamespacePreflightAndMissingPauseConfirmationDoNotMutate(t *testing.T) {
	js := legacyNamespaceFixture(t)
	if err := MigrateControlSharedNamespace(context.Background(), js, false, false, nil); err != nil {
		t.Fatal(err)
	}
	if err := MigrateControlSharedNamespace(context.Background(), js, true, false, nil); err == nil {
		t.Fatal("accepted unpaused migration")
	}
	if err := MigrateControlSharedNamespace(context.Background(), js, true, true, nil); err == nil {
		t.Fatal("accepted missing snapshot")
	}
	old, _ := js.StreamInfo(legacyControlSharedStream)
	if namespaceIsArchived(old) {
		t.Fatal("preflight changed ownership")
	}
}

func TestNamespaceMigrationRefusesAckPendingAndSnapshotFailure(t *testing.T) {
	t.Run("ack-pending", func(t *testing.T) {
		js := legacyNamespaceFixture(t)
		if _, err := js.Publish("aqencia.controlplane.user.deleted", []byte("not-acked")); err != nil {
			t.Fatal(err)
		}
		sub, err := js.PullSubscribe("aqencia.controlplane.>", "prior-consumer", nats.Bind(legacyControlSharedStream, "prior-consumer"))
		if err != nil {
			t.Fatal(err)
		}
		if _, err := sub.Fetch(1, nats.MaxWait(time.Second)); err != nil {
			t.Fatal(err)
		}
		if err := MigrateControlSharedNamespace(context.Background(), js, true, true, func(NamespaceSnapshot) error { return nil }); err == nil {
			t.Fatal("accepted unacknowledged work")
		}
		old, _ := js.StreamInfo(legacyControlSharedStream)
		if namespaceIsArchived(old) {
			t.Fatal("changed active subjects")
		}
	})
	t.Run("snapshot-write-failure", func(t *testing.T) {
		js := legacyNamespaceFixture(t)
		if err := MigrateControlSharedNamespace(context.Background(), js, true, true, func(NamespaceSnapshot) error { return context.Canceled }); err == nil {
			t.Fatal("accepted failed snapshot")
		}
		old, _ := js.StreamInfo(legacyControlSharedStream)
		if namespaceIsArchived(old) {
			t.Fatal("changed subjects before saving snapshot")
		}
	})
}
