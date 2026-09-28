package watch

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"
)

// ControlCreateAuthorizer obtains Control's `model.watch.create` decision and
// returns the authority binding to store on the row.
//
// Separate from [ControlObserveAuthorizer] because the two answer different
// questions at different times. This one runs with a HUMAN present — Control
// resolves the subject from the verified delegation rather than being told who
// it is — and its answer is recorded. The observe authorizer runs later with
// nobody present, and its answer is used once and discarded.
//
// The returned [Authority] is the point of the call as much as the approval is:
// the sweeper needs something to compare current authority AGAINST, and the
// only honest source for it is the decision that allowed the watch to exist.
type ControlCreateAuthorizer struct {
	endpoint  string
	serviceID string
	token     string
	http      *http.Client
}

// NewControlCreateAuthorizer builds the authorizer. Every field is required: an
// authorizer that cannot reach Control must fail to construct rather than
// quietly approve.
func NewControlCreateAuthorizer(endpoint, serviceID, token string, client *http.Client) (*ControlCreateAuthorizer, error) {
	endpoint = strings.TrimRight(strings.TrimSpace(endpoint), "/")
	serviceID = strings.TrimSpace(serviceID)
	token = strings.TrimSpace(token)
	if endpoint == "" || serviceID == "" || token == "" {
		return nil, fmt.Errorf("a watch create authorizer requires a Control endpoint, a service id and a service token")
	}
	if client == nil {
		client = &http.Client{Timeout: 10 * time.Second}
	}
	return &ControlCreateAuthorizer{endpoint: endpoint, serviceID: serviceID, token: token, http: client}, nil
}

// controlDecision is the slice of Control's signed envelope this service keeps.
//
// Only the authority binding, deliberately. The permissions and the token
// itself are not stored: a watch row is not a credential, and keeping one on it
// would make the row worth stealing.
type controlDecision struct {
	RecipientAudienceRef      string `json:"recipient_audience_ref"`
	RecipientAudienceHash     string `json:"recipient_audience_hash"`
	ResourceAuthorizationRef  string `json:"resource_authorization_ref"`
	PrivacyPolicyRef          string `json:"privacy_policy_ref"`
	AuthorityRevision         int64  `json:"authority_revision"`
	MembershipRevision        int64  `json:"membership_revision"`
	PrivacyRevision           int64  `json:"privacy_revision"`
	RecipientAudienceRevision int64  `json:"recipient_audience_revision"`
	EntitlementRevision       int64  `json:"entitlement_revision"`
}

// AuthorizeCreate implements the handler's CreateAuthorizer.
func (a *ControlCreateAuthorizer) AuthorizeCreate(ctx context.Context, w Watch) (Authority, error) {
	body, err := json.Marshal(map[string]any{
		"space_ref":   w.SpaceRef,
		"watch_id":    w.ID,
		"source_kind": w.SourceKind,
		"source_ref":  w.SourceRef,
		// The predicate as a DIGEST. Control does not need the matching rule to
		// enforce Space policy, and binding the digest is what stops an approved
		// watch from being re-pointed at a different rule afterwards.
		"predicate_digest": PredicateDigest(w.Predicate),
		"idempotency_key":  w.ID,
	})
	if err != nil {
		return Authority{}, fmt.Errorf("encode a watch create request: %w", err)
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost,
		a.endpoint+"/api/v1/internal/spaces/watch-create-decision", bytes.NewReader(body))
	if err != nil {
		return Authority{}, fmt.Errorf("build a watch create request: %w", err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-Service-Id", a.serviceID)
	request.Header.Set("X-Service-Token", a.token)

	response, err := a.http.Do(request)
	if err != nil {
		return Authority{}, fmt.Errorf("%w: %v", ErrAuthorityUnavailable, err)
	}
	defer func() { _ = response.Body.Close() }()
	if response.StatusCode != http.StatusOK {
		if response.StatusCode >= 500 {
			return Authority{}, fmt.Errorf("%w: Control returned status %d", ErrAuthorityUnavailable, response.StatusCode)
		}
		return Authority{}, fmt.Errorf("Control refused this watch (status %d)", response.StatusCode)
	}
	var envelope struct {
		Data struct {
			Decision controlDecision `json:"decision"`
		} `json:"data"`
	}
	if err := json.NewDecoder(response.Body).Decode(&envelope); err != nil {
		return Authority{}, fmt.Errorf("decode a watch create decision: %w", err)
	}
	decision := envelope.Data.Decision
	if strings.TrimSpace(decision.RecipientAudienceRef) == "" || decision.RecipientAudienceRevision == 0 {
		// An approval with no audience binding is not usable: the sweeper would
		// have nothing to compare against, and the adapter's ceiling check would
		// silently pass everything. Refusing beats storing a watch whose
		// authority cannot be re-derived.
		return Authority{}, fmt.Errorf("Control returned a watch decision with no recipient audience")
	}
	return Authority{
		RecipientAudienceRef:      decision.RecipientAudienceRef,
		RecipientAudienceHash:     decision.RecipientAudienceHash,
		ResourceAuthorizationRef:  decision.ResourceAuthorizationRef,
		PrivacyPolicyRef:          decision.PrivacyPolicyRef,
		AuthorityRevision:         decision.AuthorityRevision,
		MembershipRevision:        decision.MembershipRevision,
		PrivacyRevision:           decision.PrivacyRevision,
		RecipientAudienceRevision: decision.RecipientAudienceRevision,
		EntitlementRevision:       decision.EntitlementRevision,
	}, nil
}
