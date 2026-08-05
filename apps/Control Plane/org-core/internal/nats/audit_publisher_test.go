package nats

import (
	"context"
	"errors"
	"strings"
	"testing"

	gonats "github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"
)

type auditAckPublisherStub struct {
	ack *jetstream.PubAck
	err error
	msg *gonats.Msg
}

func (p *auditAckPublisherStub) Publish(_ context.Context, _ string, _ []byte, _ ...jetstream.PublishOpt) (*jetstream.PubAck, error) {
	return p.ack, p.err
}

func (p *auditAckPublisherStub) PublishMsg(_ context.Context, msg *gonats.Msg, _ ...jetstream.PublishOpt) (*jetstream.PubAck, error) {
	p.msg = msg
	return p.ack, p.err
}

func TestPublishAuditMessageRequiresPubAckAndPinsStableMessageID(t *testing.T) {
	if err := publishAuditMessage(context.Background(), nil, "subject", "event", []byte(`{}`)); err == nil {
		t.Fatal("nil JetStream publisher was accepted")
	}
	if err := publishAuditMessage(context.Background(), &auditAckPublisherStub{}, "", "event", []byte(`{}`)); err == nil {
		t.Fatal("empty audit subject was accepted")
	}
	tests := []struct {
		name    string
		ack     *jetstream.PubAck
		err     error
		wantErr string
	}{
		{name: "transport failure", err: errors.New("broker unavailable"), wantErr: "broker unavailable"},
		{name: "missing acknowledgement", wantErr: "invalid JetStream PubAck"},
		{name: "empty acknowledgement", ack: &jetstream.PubAck{}, wantErr: "invalid JetStream PubAck"},
		{name: "acknowledged", ack: &jetstream.PubAck{Stream: "VEREVON_CONTROL_OBSERVABILITY", Sequence: 42}},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			publisher := &auditAckPublisherStub{ack: tt.ack, err: tt.err}
			err := publishAuditMessage(
				context.Background(),
				publisher,
				"verevon.audit.v2.control.org-core.erasure",
				"gdpr:org-core:stable-event",
				[]byte(`{"event_id":"gdpr:org-core:stable-event"}`),
			)
			if tt.wantErr != "" {
				if err == nil || !strings.Contains(err.Error(), tt.wantErr) {
					t.Fatalf("publish error=%v; want %q", err, tt.wantErr)
				}
				return
			}
			if err != nil {
				t.Fatalf("publish audit: %v", err)
			}
			if publisher.msg == nil {
				t.Fatal("publisher did not receive a message")
			}
			if got := publisher.msg.Header.Get(gonats.MsgIdHdr); got != "gdpr:org-core:stable-event" {
				t.Fatalf("Nats-Msg-Id=%q; want stable event identity", got)
			}
			if got := publisher.msg.Subject; got != "verevon.audit.v2.control.org-core.erasure" {
				t.Fatalf("subject=%q", got)
			}
		})
	}
}

func TestClientPublishAuditEncodesPayloadAndUsesAckPublisher(t *testing.T) {
	publisher := &auditAckPublisherStub{
		ack: &jetstream.PubAck{Stream: "VEREVON_CONTROL_OBSERVABILITY", Sequence: 9},
	}
	client := &Client{js: publisher}
	if err := client.PublishAudit(
		context.Background(), "verevon.audit.v2.control.org-core.erasure",
		"gdpr:org-core:client", map[string]any{"event_id": "gdpr:org-core:client"},
	); err != nil {
		t.Fatalf("publish client audit: %v", err)
	}
	if publisher.msg == nil || !strings.Contains(string(publisher.msg.Data), "gdpr:org-core:client") {
		t.Fatalf("published message=%v", publisher.msg)
	}
	if err := client.PublishAudit(
		context.Background(), "verevon.audit.v2.control.org-core.erasure",
		"gdpr:org-core:bad", map[string]any{"bad": func() {}},
	); err == nil || !strings.Contains(err.Error(), "marshal") {
		t.Fatalf("unencodable audit payload error=%v", err)
	}
}
