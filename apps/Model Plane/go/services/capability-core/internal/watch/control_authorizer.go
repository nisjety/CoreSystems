package watch

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"
)

// ControlObserveAuthorizer re-derives current Space authority from Control
// immediately before a watch records anything a human can read.
//
// # Why this is attached to the DISCLOSURE and not to the read
//
// Step 1 checked authority before every poll. That is stricter and, at a
// two-second cadence, far more expensive than the cron sweeper it copies — one
// Control call per watch per two seconds, where cron makes one per fire.
//
// The check belongs where the disclosure is. A poll that matches nothing reads
// into the sweeper's memory on capability-core's own service credential, bound
// to the watch's Space by the adapter, and discards it: no human sees anything
// and nothing durable is written. What needs fresh human authority is turning
// that read into a recorded, readable event. So the check sits on the emission,
// and a quiet watch costs Control nothing.
//
// The cost of that placement, stated rather than hidden: a member whose
// membership was revoked keeps polling until their watch's first would-be
// event, rather than until its next poll. Nothing is disclosed in the meantime,
// and `expires_at` bounds it in the worst case — but if that window ever needs
// closing, the fix is a periodic revalidation sweep, not moving this check back
// in front of every read.
type ControlObserveAuthorizer struct {
	endpoint  string
	serviceID string
	token     string
	http      *http.Client
}

// NewControlObserveAuthorizer builds the authorizer from deployment
// configuration. Every field is required: an authorizer that cannot reach
// Control must fail to construct rather than quietly authorize.
func NewControlObserveAuthorizer(endpoint, serviceID, token string, client *http.Client) (*ControlObserveAuthorizer, error) {
	endpoint = strings.TrimRight(strings.TrimSpace(endpoint), "/")
	serviceID = strings.TrimSpace(serviceID)
	token = strings.TrimSpace(token)
	if endpoint == "" || serviceID == "" || token == "" {
		return nil, fmt.Errorf("a watch observe authorizer requires a Control endpoint, a service id and a service token")
	}
	if client == nil {
		client = &http.Client{Timeout: 10 * time.Second}
	}
	return &ControlObserveAuthorizer{endpoint: endpoint, serviceID: serviceID, token: token, http: client}, nil
}

// PredicateDigest is the content-free identity of a watch's matching rule.
//
// Control binds this into both watch decisions so an approved predicate cannot
// be swapped afterwards — a watch approved for "tell me when it says ERROR"
// must not become "tell me everything", which for a watch is the difference
// between a notification and a transcript.
//
// Length-prefixed like every other digest in this repo: without it, a
// `contains` predicate for "a" on stream "bc" and one for "ab" on stream "c"
// would hash identically.
func PredicateDigest(p Predicate) string {
	hash := sha256.New()
	hash.Write([]byte("space.watch.predicate\x00v1\x00"))
	for _, field := range []string{p.Kind, p.Value, p.Stream} {
		var length [8]byte
		for i := 0; i < 8; i++ {
			length[7-i] = byte(len(field) >> (8 * i))
		}
		hash.Write(length[:])
		hash.Write([]byte(field))
	}
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}

type observeIntent struct {
	OrgID           string `json:"org_id"`
	SpaceRef        string `json:"space_ref"`
	SubjectID       string `json:"subject_id"`
	WatchID         string `json:"watch_id"`
	SourceKind      string `json:"source_kind"`
	SourceRef       string `json:"source_ref"`
	PredicateDigest string `json:"predicate_digest"`
	IdempotencyKey  string `json:"idempotency_key"`
}

// AuthorizeObserve implements [Reauthorizer].
//
// Every field it sends comes from the watch ROW, and Control re-resolves
// membership, privacy, audience and resource authority from its own store
// before signing. The row is evidence of what was once true; Control is the
// authority on what is true now.
//
// The matched content is deliberately absent from the request. Control
// authorizes the disclosure, it does not review it, and sending a program's
// output to the identity plane would put unscreened payload somewhere it has no
// business being.
func (a *ControlObserveAuthorizer) AuthorizeObserve(ctx context.Context, w Watch) error {
	intent := observeIntent{
		OrgID: w.OrgID, SpaceRef: w.SpaceRef, SubjectID: w.CreatorSubjectID,
		WatchID: w.ID, SourceKind: w.SourceKind, SourceRef: w.SourceRef,
		PredicateDigest: PredicateDigest(w.Predicate),
		// The cursor makes this key stable for one position and different for
		// the next, so a retried observation at the same position asks the same
		// question rather than a new one.
		IdempotencyKey: fmt.Sprintf("%s:%d", w.ID, w.CursorValue),
	}
	body, err := json.Marshal(map[string]any{"intent": intent})
	if err != nil {
		return fmt.Errorf("encode a watch observe intent: %w", err)
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost,
		a.endpoint+"/api/v1/internal/spaces/watch-observe-decision", bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("build a watch observe request: %w", err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-Service-Id", a.serviceID)
	request.Header.Set("X-Service-Token", a.token)

	response, err := a.http.Do(request)
	if err != nil {
		// A transport failure is NOT a refusal. Returning an error here
		// terminates the watch, and Control being briefly unreachable must not
		// cancel a person's watch — so this is wrapped as a distinct condition
		// the sweeper backs off on.
		return fmt.Errorf("%w: %v", ErrAuthorityUnavailable, err)
	}
	defer func() { _ = response.Body.Close() }()
	switch {
	case response.StatusCode == http.StatusOK:
		return nil
	case response.StatusCode == http.StatusForbidden || response.StatusCode == http.StatusNotFound:
		// A real refusal: the member's authority no longer covers this watch.
		return fmt.Errorf("Control refused this watch's observation (status %d)", response.StatusCode)
	default:
		// Anything else — 5xx, a gateway timeout, a misconfigured route — is
		// Control being unable to answer rather than answering no.
		return fmt.Errorf("%w: Control returned status %d", ErrAuthorityUnavailable, response.StatusCode)
	}
}
