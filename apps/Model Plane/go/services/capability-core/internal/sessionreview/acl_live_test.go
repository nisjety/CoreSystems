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

// Env vars carrying the two credentialled URLs, set by
// scripts/tests/learning-acl-grant-test.sh.
const (
	aclPublisherURLEnv  = "NATS_ACL_SESSION_CORE_URL"
	aclSubscriberURLEnv = "NATS_ACL_CAPABILITY_CORE_URL"
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
