package nats

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/nats-io/nats.go/jetstream"
)

// WS04-S10: PublishWithAck waits for the JetStream PubAck, so the membership
// outbox keeps a row pending until the broker has really accepted it.

func TestPublishWithAckRequiresAValidPubAck(t *testing.T) {
	for _, test := range []struct {
		name    string
		ack     *jetstream.PubAck
		err     error
		wantErr bool
	}{
		{name: "acknowledged", ack: &jetstream.PubAck{Stream: "CONTROLPLANE", Sequence: 7}},
		{name: "transport failure", err: errors.New("broker unavailable"), wantErr: true},
		{name: "nil ack", wantErr: true},
		{name: "wrong stream", ack: &jetstream.PubAck{Stream: "OTHER", Sequence: 7}, wantErr: true},
		{name: "zero sequence", ack: &jetstream.PubAck{Stream: "CONTROLPLANE"}, wantErr: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			fake := &fakeGDPRJetStream{ack: test.ack, err: test.err}
			publisher := &SharedPublisher{gdprJS: fake, sourceName: "user-core"}
			err := publisher.PublishWithAck(context.Background(), "controlplane.space.membership_changed", "evt-1", map[string]any{"space_ref": "s1"})
			if test.wantErr && err == nil {
				t.Fatal("publish without a valid PubAck was accepted")
			}
			if !test.wantErr && err != nil {
				t.Fatalf("valid PubAck rejected: %v", err)
			}
			if test.err != nil && !errors.Is(err, test.err) {
				t.Fatalf("transport error was not wrapped: %v", err)
			}
		})
	}
}

func TestPublishWithAckSetsTheDedupHeaderAndEnrichesThePayload(t *testing.T) {
	fake := &fakeGDPRJetStream{ack: &jetstream.PubAck{Stream: "CONTROLPLANE", Sequence: 1}}
	publisher := &SharedPublisher{gdprJS: fake, sourceName: "user-core"}
	before := time.Now().UTC().Add(-time.Second)
	if err := publisher.PublishWithAck(context.Background(), "controlplane.space.membership_changed", "evt-42",
		map[string]any{"space_ref": "s1", "active": true, "space_seq": 9}); err != nil {
		t.Fatal(err)
	}
	if fake.msg == nil {
		t.Fatal("nothing was published")
	}
	if fake.msg.Subject != "controlplane.space.membership_changed" {
		t.Fatalf("subject = %q", fake.msg.Subject)
	}
	if got := fake.msg.Header.Get("Nats-Msg-Id"); got != "evt-42" {
		t.Fatalf("Nats-Msg-Id = %q, want evt-42", got)
	}
	var body map[string]any
	if err := json.Unmarshal(fake.msg.Data, &body); err != nil {
		t.Fatalf("payload is not JSON: %v", err)
	}
	if body["space_ref"] != "s1" || body["active"] != true || body["space_seq"] != float64(9) {
		t.Fatalf("payload lost its fields: %v", body)
	}
	if body["_source"] != "user-core" {
		t.Fatalf("_source = %v", body["_source"])
	}
	published, err := time.Parse(time.RFC3339Nano, body["_published_at"].(string))
	if err != nil || published.Before(before) {
		t.Fatalf("_published_at = %v err=%v", body["_published_at"], err)
	}
}

func TestPublishWithAckDoesNotMutateTheCallersPayload(t *testing.T) {
	publisher := &SharedPublisher{gdprJS: &fakeGDPRJetStream{ack: &jetstream.PubAck{Stream: "CONTROLPLANE", Sequence: 1}}, sourceName: "user-core"}
	payload := map[string]any{"space_ref": "s1"}
	if err := publisher.PublishWithAck(context.Background(), "controlplane.x", "evt-1", payload); err != nil {
		t.Fatal(err)
	}
	if len(payload) != 1 {
		t.Fatalf("the caller's map gained keys: %v", payload)
	}
}

func TestPublishWithAckRefusesEmptyIdentityAndAnUnavailableBroker(t *testing.T) {
	fake := &fakeGDPRJetStream{ack: &jetstream.PubAck{Stream: "CONTROLPLANE", Sequence: 1}}
	publisher := &SharedPublisher{gdprJS: fake}
	for name, args := range map[string][2]string{
		"empty subject":    {"", "evt-1"},
		"blank subject":    {"  ", "evt-1"},
		"empty message id": {"controlplane.x", ""},
		"blank message id": {"controlplane.x", "\t"},
	} {
		t.Run(name, func(t *testing.T) {
			fake.msg = nil
			if err := publisher.PublishWithAck(context.Background(), args[0], args[1], map[string]any{}); err == nil {
				t.Fatal("accepted")
			}
			if fake.msg != nil {
				t.Fatal("a message was sent despite the refusal")
			}
		})
	}
	var nilPublisher *SharedPublisher
	if err := nilPublisher.PublishWithAck(context.Background(), "controlplane.x", "evt-1", nil); err == nil ||
		!strings.Contains(err.Error(), "unavailable") {
		t.Fatalf("nil publisher: err = %v, want an unavailable error", err)
	}
	if err := (&SharedPublisher{}).PublishWithAck(context.Background(), "controlplane.x", "evt-1", nil); err == nil {
		t.Fatal("a publisher without JetStream accepted a publish")
	}
}

func TestPublishWithAckRefusesAnUnmarshalablePayload(t *testing.T) {
	fake := &fakeGDPRJetStream{ack: &jetstream.PubAck{Stream: "CONTROLPLANE", Sequence: 1}}
	publisher := &SharedPublisher{gdprJS: fake}
	if err := publisher.PublishWithAck(context.Background(), "controlplane.x", "evt-1", map[string]any{"bad": make(chan int)}); err == nil {
		t.Fatal("an unmarshalable payload was accepted")
	}
	if fake.msg != nil {
		t.Fatal("a message was sent despite the marshal failure")
	}
}
