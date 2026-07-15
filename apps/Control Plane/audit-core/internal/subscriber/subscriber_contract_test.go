package subscriber

import (
	"os"
	"strings"
	"testing"
)

func TestSubscriberUsesDurableJetStreamWithExplicitFailureDisposition(t *testing.T) {
	sourceBytes, err := os.ReadFile("subscriber.go")
	if err != nil {
		t.Fatalf("read subscriber source: %v", err)
	}
	source := string(sourceBytes)

	for _, required := range []string{
		"JetStream()",
		"QueueSubscribe",
		"nats.Bind",
		"nats.ManualAck",
		"nats.AckExplicit",
		"nats.MaxDeliver",
		"msg.Ack()",
		"msg.NakWithDelay",
		"msg.Term()",
		"dead_lettered",
		"if !s.deadLetter",
		"InsertAuditFromStream",
		"InsertUsageFromStream",
		"planeSubject",
		"audit-core-%s-v3-%s",
	} {
		if !strings.Contains(source, required) {
			t.Errorf("durable subscriber contract missing %q", required)
		}
	}
	if strings.Contains(source, "s.nc.QueueSubscribe") {
		t.Error("core NATS queue subscription remains enabled")
	}
	for _, forbidden := range []string{"AddStream", "UpdateStream", "nats.Durable"} {
		if strings.Contains(source, forbidden) {
			t.Errorf("audit runtime retains JetStream administration capability %q", forbidden)
		}
	}
}
