package events

import (
	"errors"
	"testing"

	"github.com/nats-io/nats.go"
)

type fakeJetStreamPublisher struct {
	ack   *nats.PubAck
	err   error
	calls int
	opts  int
}

func (f *fakeJetStreamPublisher) Publish(_ string, _ []byte, opts ...nats.PubOpt) (*nats.PubAck, error) {
	f.calls++
	f.opts = len(opts)
	return f.ack, f.err
}

type fakeEventSigner struct{ err error }

func (f fakeEventSigner) Sign(_ string, payload []byte) ([]byte, error) {
	if f.err != nil {
		return nil, f.err
	}
	return append([]byte("signed:"), payload...), nil
}

func TestOutboxRequiresJetStreamAcknowledgementBeforeDelivery(t *testing.T) {
	if publisher, err := NewOutboxPublisher(nil, nil, nil); err == nil || publisher != nil {
		t.Fatal("outbox constructor accepted unavailable production dependencies")
	}
	t.Run("acknowledged", func(t *testing.T) {
		js := &fakeJetStreamPublisher{ack: &nats.PubAck{Stream: "DATAPLANE_DOCUMENTS", Sequence: 7}}
		publisher := &OutboxPublisher{js: js, signer: fakeEventSigner{}}
		if err := publisher.publishAcknowledged("dataplane.documents.deleted", []byte(`{"org_id":"org-a","zdr":false}`), "documents-outbox-7"); err != nil {
			t.Fatalf("acknowledged publish failed: %v", err)
		}
		if js.calls != 1 {
			t.Fatalf("publish calls=%d, want 1", js.calls)
		}
		if js.opts != 1 {
			t.Fatalf("publish options=%d, want stable message id", js.opts)
		}
	})

	for _, tc := range []struct {
		name   string
		js     *fakeJetStreamPublisher
		signer fakeEventSigner
	}{
		{name: "signing fails", js: &fakeJetStreamPublisher{}, signer: fakeEventSigner{err: errors.New("sign")}},
		{name: "broker rejects", js: &fakeJetStreamPublisher{err: errors.New("broker")}, signer: fakeEventSigner{}},
		{name: "missing ack", js: &fakeJetStreamPublisher{}, signer: fakeEventSigner{}},
		{name: "empty stream ack", js: &fakeJetStreamPublisher{ack: &nats.PubAck{}}, signer: fakeEventSigner{}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			publisher := &OutboxPublisher{js: tc.js, signer: tc.signer}
			if err := publisher.publishAcknowledged("dataplane.documents.deleted", []byte(`{}`), "documents-outbox-8"); err == nil {
				t.Fatal("unacknowledged publish was accepted")
			}
		})
	}
}
