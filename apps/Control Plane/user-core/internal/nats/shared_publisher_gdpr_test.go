package nats

import (
	"context"
	"errors"
	"testing"

	gonats "github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"
)

type fakeGDPRJetStream struct {
	ack *jetstream.PubAck
	err error
	msg *gonats.Msg
}

func (f *fakeGDPRJetStream) PublishMsg(_ context.Context, msg *gonats.Msg, _ ...jetstream.PublishOpt) (*jetstream.PubAck, error) {
	f.msg = msg
	return f.ack, f.err
}

func TestPublishGDPRErasureRequiresValidJetStreamPubAck(t *testing.T) {
	for _, test := range []struct {
		name    string
		ack     *jetstream.PubAck
		err     error
		wantErr bool
	}{
		{name: "acknowledged", ack: &jetstream.PubAck{Stream: "AQENCIA_CONTROLPLANE", Sequence: 42}},
		{name: "transport failure", err: errors.New("broker unavailable"), wantErr: true},
		{name: "missing ack", wantErr: true},
		{name: "wrong stream", ack: &jetstream.PubAck{Stream: "OTHER", Sequence: 42}, wantErr: true},
		{name: "zero sequence", ack: &jetstream.PubAck{Stream: "AQENCIA_CONTROLPLANE"}, wantErr: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			publisher := &SharedPublisher{gdprJS: &fakeGDPRJetStream{ack: test.ack, err: test.err}}
			err := publisher.PublishGDPRErasure(context.Background(), "gdpr:fanout:stable", []byte(`{"event_id":"gdpr:fanout:stable"}`))
			if test.wantErr && err == nil {
				t.Fatal("publish without a valid PubAck was accepted")
			}
			if !test.wantErr && err != nil {
				t.Fatalf("valid PubAck rejected: %v", err)
			}
		})
	}
}
