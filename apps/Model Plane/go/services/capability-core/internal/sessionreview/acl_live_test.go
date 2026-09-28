package sessionreview

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/triodelab/model-plane/pkg/envelope"
)

// Env vars carrying the credentialled URLs, set by
// scripts/tests/learning-acl-grant-test.sh.
const (
	aclPublisherURLEnv   = "NATS_ACL_SESSION_CORE_URL"
	aclSubscriberURLEnv  = "NATS_ACL_CAPABILITY_CORE_URL"
	aclProvisionerURLEnv = "NATS_ACL_PROVISIONER_URL"
)

// aclNegativeControlSubject is a subject session-core-runtime must NOT hold.
// `mp.v1.capability.>` belongs to capability-core, so a server that accepts
// this publish is not enforcing per-principal grants at all.
const aclNegativeControlSubject = "mp.v1.capability.acl-negative-control"

// TestRunEventACLGrant_ProductionConfig proves that the PRODUCTION NATS
// authorization block (deploy/nats.conf) actually permits the one publish the
// G7 learning loop depends on, and that it is enforcing grants while doing so.
//
// # Why this is separate from TestRunConsumerAgainstLiveNATS
//
// That test runs against an unauthenticated disposable server, so it proves
// subject routing, delivery, and the review→persist path. It cannot fail the
// way this loop actually failed before: a correct producer, a correct consumer,
// a correct subject — and a broker that silently rejects the publish because
// the principal was never granted it. NATS reports a publish-permission denial
// to the PUBLISHER's async error handler and drops the message; the publisher's
// `Publish()` call still returns nil. So both services log success, the
// drainer burns its attempts to terminal_at, and no skill is ever learned.
//
// The failure is invisible to every in-process test, which is why it is pinned
// here against the real config file with the real principals.
//
// # The negative control is not optional
//
// A wide-open server passes the positive assertion trivially. Publishing to a
// subject session-core-runtime does NOT hold must be denied, or this test is
// vacuous and would keep passing after someone replaced the ACL with
// `publish: [">"]`.
func TestRunEventACLGrant_ProductionConfig(t *testing.T) {
	pubURL := os.Getenv(aclPublisherURLEnv)
	subURL := os.Getenv(aclSubscriberURLEnv)
	if pubURL == "" || subURL == "" {
		t.Skipf("requires %s and %s (run scripts/tests/learning-acl-grant-test.sh)",
			aclPublisherURLEnv, aclSubscriberURLEnv)
	}

	// The consumer half, on capability-core's own credential and its own
	// production subject filter.
	subConn, err := nats.Connect(subURL)
	if err != nil {
		t.Fatalf("connect as capability-core-runtime: %v", err)
	}
	defer subConn.Close()

	delivered := make(chan *nats.Msg, 4)
	sub, err := subConn.Subscribe(RunCompletedSubject, func(msg *nats.Msg) {
		delivered <- msg
	})
	if err != nil {
		t.Fatalf("capability-core-runtime cannot subscribe %s: %v", RunCompletedSubject, err)
	}
	defer func() { _ = sub.Unsubscribe() }()
	if err := subConn.Flush(); err != nil {
		t.Fatalf("flush subscriber: %v", err)
	}

	// The producer half, on session-core's own credential. Permission denials
	// arrive asynchronously, so they must be captured — not awaited on the
	// Publish() return, which is nil either way.
	asyncErrs := make(chan error, 8)
	pubConn, err := nats.Connect(pubURL, nats.ErrorHandler(
		func(_ *nats.Conn, _ *nats.Subscription, aerr error) {
			asyncErrs <- aerr
		}))
	if err != nil {
		t.Fatalf("connect as session-core-runtime: %v", err)
	}
	defer pubConn.Close()

	// A ULID-shaped run id: `mp.v1.run.*.event` matches exactly one token, so a
	// run id containing a dot would publish where nothing is listening. Real
	// run ids are ULID-prefixed and dot-free; asserting on one keeps this test
	// honest about the shape the grant actually covers.
	const runID = "01JTESTACLGRANT0000000000"
	subject := fmt.Sprintf("mp.v1.run.%s.event", runID)

	body := envBytes(t, RunCompletedEventType, "run/"+runID, "org-acl", `{"thread_id":"thread-acl"}`)
	if err := pubConn.Publish(subject, body); err != nil {
		t.Fatalf("publish %s: %v", subject, err)
	}
	if err := pubConn.Flush(); err != nil {
		t.Fatalf("flush publisher: %v", err)
	}

	select {
	case msg := <-delivered:
		if msg.Subject != subject {
			t.Fatalf("delivered on %s, want %s", msg.Subject, subject)
		}
		// Decode through the real trigger so a field-name drift between the
		// Rust producer envelope and Go's struct cannot pass as "delivered".
		ref, ok := parseDeliveredEnvelope(t, msg.Data)
		if !ok {
			t.Fatalf("delivered envelope is not a RUN_COMPLETED the trigger accepts: %s", msg.Data)
		}
		if ref.RunID != runID {
			t.Fatalf("parsed run id %q, want %q", ref.RunID, runID)
		}
	case aerr := <-asyncErrs:
		t.Fatalf("session-core-runtime is NOT granted %s: %v\n"+
			"Add it to the session-core-runtime publish list in deploy/nats.conf — "+
			"without the grant the learning loop is inert and nothing logs an error.",
			subject, aerr)
	case <-time.After(3 * time.Second):
		t.Fatalf("no delivery of %s within 3s and no permission error either", subject)
	}

	// Negative control: the same principal, a subject it does not hold.
	if err := pubConn.Publish(aclNegativeControlSubject, []byte(`{}`)); err != nil {
		t.Fatalf("publish negative control: %v", err)
	}
	if err := pubConn.Flush(); err != nil {
		t.Fatalf("flush negative control: %v", err)
	}
	select {
	case aerr := <-asyncErrs:
		if !strings.Contains(strings.ToLower(aerr.Error()), "permissions violation") {
			t.Fatalf("expected a permissions violation for %s, got: %v",
				aclNegativeControlSubject, aerr)
		}
	case <-time.After(3 * time.Second):
		t.Fatalf("session-core-runtime was ALLOWED to publish %s — the server is not "+
			"enforcing per-principal grants, so this test's positive assertion is vacuous",
			aclNegativeControlSubject)
	}
}

// parseDeliveredEnvelope runs delivered bytes through the production trigger
// parser, so "it arrived on the wire" and "the consumer would act on it" are a
// single assertion. A Rust-producer field-name drift would deliver fine and
// parse to nothing — exactly the silent failure this file exists to catch.
func parseDeliveredEnvelope(t *testing.T, data []byte) (SessionRef, bool) {
	t.Helper()
	var env envelope.Envelope
	if err := json.Unmarshal(data, &env); err != nil {
		t.Fatalf("delivered bytes are not a decodable envelope: %v", err)
	}
	return ParseRunCompleted(&env)
}

// TestSkillReviewJetStreamACLGrant_ProductionConfig proves the ADDITIONAL grant
// this package's 2026-09-17 fix depends on: capability-core-runtime's
// $JS.API.CONSUMER.INFO / $JS.ACK / deliver-subject permissions, without which
// RunConsumer's nats.Bind hangs until "context deadline exceeded" and the
// server logs a Permissions Violation the caller never sees (a permission
// denial on $JS.API.* is reported the same asynchronous, silent way as the
// plain-publish denial TestRunEventACLGrant_ProductionConfig above documents).
//
// This is deliberately a SEPARATE test from that one: the older test proves the
// plain "mp.v1.run.*.event" subscribe grant taskexec.RunCompletionConsumer
// still needs; this one proves the JetStream-bind grant sessionreview and
// runwatch need, which is a disjoint set of subjects added specifically for
// this fix. Passing one does not imply the other.
func TestSkillReviewJetStreamACLGrant_ProductionConfig(t *testing.T) {
	pubURL := os.Getenv(aclPublisherURLEnv)
	subURL := os.Getenv(aclSubscriberURLEnv)
	provURL := os.Getenv(aclProvisionerURLEnv)
	if pubURL == "" || subURL == "" || provURL == "" {
		t.Skipf("requires %s, %s and %s (run scripts/tests/learning-acl-grant-test.sh)",
			aclPublisherURLEnv, aclSubscriberURLEnv, aclProvisionerURLEnv)
	}

	// Only observability-provisioner-model may create streams/consumers
	// (deploy/nats.conf grants it $JS.API.STREAM.>/$JS.API.CONSUMER.> and
	// nothing else holds that) — mirroring nats-provisioner's own real
	// contract, not a test-only shortcut. Its subscribe grant is scoped to
	// "_INBOX.PROVISIONER_MODEL.>", not the default random inbox prefix a
	// bare nats.Connect uses for JetStream API replies — the real
	// nats-provisioner binary sets this same option (services/
	// nats-provisioner/main.go); a test connection that omits it gets a
	// Permissions Violation on its OWN reply subject, not on anything this
	// fix touches.
	provConn, err := nats.Connect(provURL, nats.CustomInboxPrefix("_INBOX.PROVISIONER_MODEL"))
	if err != nil {
		t.Fatalf("connect as observability-provisioner-model: %v", err)
	}
	defer provConn.Close()
	js, err := provConn.JetStream()
	if err != nil {
		t.Fatalf("provisioner JetStream context: %v", err)
	}
	if _, err := js.AddStream(&nats.StreamConfig{
		Name:      RunEventsStream,
		Subjects:  []string{RunCompletedSubject},
		Retention: nats.LimitsPolicy,
		Storage:   nats.MemoryStorage,
		MaxAge:    time.Hour,
	}); err != nil {
		t.Fatalf("provisioner add stream: %v", err)
	}
	if _, err := js.AddConsumer(RunEventsStream, &nats.ConsumerConfig{
		Durable:        SkillReviewDurable,
		DeliverSubject: "_VEREVON.MODEL.DELIVER.capability.skill_review",
		DeliverGroup:   SkillReviewDurable,
		AckPolicy:      nats.AckExplicitPolicy,
		AckWait:        30 * time.Second,
		MaxDeliver:     5,
		FilterSubject:  RunCompletedSubject,
		DeliverPolicy:  nats.DeliverAllPolicy,
	}); err != nil {
		t.Fatalf("provisioner add consumer: %v", err)
	}

	// The consumer half, on capability-core-runtime's own credential — the
	// exact call sessionreview.RunConsumer makes, against the real ACL.
	// nats.Bind issues a $JS.API.CONSUMER.INFO request internally and needs
	// somewhere permitted to receive its reply, so this must match the same
	// CustomInboxPrefix production capability-core actually connects with
	// (cmd/main.go's natsInboxPrefix, "_INBOX.CAPABILITY_CORE_RUNTIME" —
	// exactly the grant this account holds) — a bare Connect uses a random
	// default inbox the ACL was never asked to permit, and fails exactly the
	// same way the provisioner connection above did before that same fix.
	subConn, err := nats.Connect(subURL, nats.CustomInboxPrefix("_INBOX.CAPABILITY_CORE_RUNTIME"))
	if err != nil {
		t.Fatalf("connect as capability-core-runtime: %v", err)
	}
	defer subConn.Close()
	subJS, err := subConn.JetStream()
	if err != nil {
		t.Fatalf("capability-core-runtime JetStream context: %v", err)
	}

	acked := make(chan struct{}, 1)
	sub, err := subJS.QueueSubscribe(RunCompletedSubject, SkillReviewDurable, func(msg *nats.Msg) {
		if aerr := msg.Ack(); aerr != nil {
			t.Errorf("capability-core-runtime is not granted $JS.ACK for its own consumer: %v", aerr)
			return
		}
		acked <- struct{}{}
	}, nats.Bind(RunEventsStream, SkillReviewDurable), nats.ManualAck())
	if err != nil {
		t.Fatalf("capability-core-runtime cannot bind %s/%s (missing $JS.API.CONSUMER.INFO grant?): %v",
			RunEventsStream, SkillReviewDurable, err)
	}
	defer func() { _ = sub.Unsubscribe() }()

	// The producer half, on session-core's own credential — proves the
	// deliver-subject subscribe grant is what actually lets the message
	// reach capability-core-runtime, not just that the bind call succeeded.
	// A JetStream Publish (unlike core NATS Publish) awaits a PubAck reply,
	// so this connection needs the same real custom inbox prefix session-
	// core's own Rust client sets (nats_connection.rs's NATS_INBOX_PREFIX,
	// "_INBOX.SESSION_CORE_RUNTIME") for the same reason as the two fixes
	// above.
	pubConn, err := nats.Connect(pubURL, nats.CustomInboxPrefix("_INBOX.SESSION_CORE_RUNTIME"))
	if err != nil {
		t.Fatalf("connect as session-core-runtime: %v", err)
	}
	defer pubConn.Close()
	pubJS, err := pubConn.JetStream()
	if err != nil {
		t.Fatalf("session-core-runtime JetStream context: %v", err)
	}
	if _, err := pubJS.Publish(RunCompletedSubject, envBytes(t, RunCompletedEventType, "run/acl-js", "org-acl-js", `{"thread_id":"thread-acl-js"}`)); err != nil {
		t.Fatalf("session-core-runtime cannot JetStream-publish %s: %v", RunCompletedSubject, err)
	}

	select {
	case <-acked:
		// Delivered AND acked through the real production ACL end to end.
	case <-time.After(3 * time.Second):
		t.Fatalf("no delivery/ack of %s within 3s — capability-core-runtime is missing a grant this fix needs "+
			"($JS.API.CONSUMER.INFO.%s.%s, $JS.ACK.%s.%s.>, or subscribe on _VEREVON.MODEL.DELIVER.capability.skill_review) "+
			"in deploy/nats.conf", RunCompletedSubject, RunEventsStream, SkillReviewDurable, RunEventsStream, SkillReviewDurable)
	}
}
