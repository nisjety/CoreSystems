package spaces

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"net"
	"sort"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"
)

// WS20-S1: the room-scoped browser run decision (model.space.browser_run).
//
// A browser run is an agent action that starts a browser session for a Space and
// writes a result turn. Control decides whether the Space may start one and signs a
// short-lived decision that the model-gateway verifier checks before it starts the
// run (WS20-S2). The decision binds the goal by digest, the canonical domain policy,
// the view scope and the run limits. It never carries the goal text. browser_entitled
// is deny by default, the agent_computer preset is the only one that turns it on, and
// Space zero data retention refuses every browser run decision (ZDR inert).
const (
	browserRunAction   = "model.space.browser_run"
	browserRunAudience = "model-plane-model-gateway"
	browserRunSchema   = "sha256:space-browser-run-v1"
	// browserRunDigestHeader opens the payload digest. The model-gateway verifier
	// recomputes the same bytes, so the header is part of the wire contract.
	browserRunDigestHeader = "model.space.browser_run\x00v1\x00"

	browserPermissionRun      = "space:browser:run"
	browserPermissionViewRoom = "space:browser:view_room"

	// BrowserProfileEphemeral is the only browser profile a room run may use.
	BrowserProfileEphemeral = "ephemeral"
	// BrowserViewOwner is the view of the run's starter alone. BrowserViewRoom also
	// lets the room see the run's view; it is never granted to a personal Space.
	BrowserViewOwner = "owner"
	BrowserViewRoom  = "room"

	// SD-23 fixes the v1 limits: at most 10 minutes and 8 hosts per run. The step
	// bounds are the gateway's R1 range (default 20).
	browserMinSteps               = 5
	browserMaxSteps               = 40
	browserMinRuntimeS            = 30
	browserMaxRuntimeS            = 600
	browserMaxHosts               = 8
	browserMaxHostBytes           = 253
	browserMaxLabelBytes          = 63
	browserMaxThreadIDBytes       = 256
	browserMaxIdempotencyKeyBytes = 256
)

// BrowserRunDecisionRequest carries only the intent the verified gateway path
// supplies. It names no actor, organisation, role, audience, policy or revision.
// ThreadID is "" for a run that starts in a new thread. The goal reaches Control only
// as its SHA-256 commitment, never as text. DecisionRef and Nonce are minted by the
// route, not sent by the caller.
type BrowserRunDecisionRequest struct {
	DecisionRef    string
	ThreadID       string
	GoalDigest     string
	AllowedDomains []string
	ViewScope      string
	MaxSteps       int
	MaxRuntimeS    int
	IdempotencyKey string
	Nonce          string
}

// Validate checks the intent's shape before any authority is read: the required
// references, the bounded identifiers, the goal commitment, the domain policy, the
// view scope and the run limits. Surrounding whitespace is refused rather than
// trimmed, so the digest binds exactly the bytes the caller sent.
func (r BrowserRunDecisionRequest) Validate() error {
	for label, value := range map[string]string{
		"decision_ref":    r.DecisionRef,
		"idempotency_key": r.IdempotencyKey,
		"nonce":           r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("browser run decision %s is required", label)
		}
	}
	if !browserTokenOK(r.IdempotencyKey, browserMaxIdempotencyKeyBytes) || !browserTokenOK(r.ThreadID, browserMaxThreadIDBytes) {
		return fmt.Errorf("browser run decision identifiers are invalid")
	}
	if !validBrowserGoalDigest(r.GoalDigest) {
		return fmt.Errorf("browser run decision goal_digest is invalid")
	}
	if err := validateBrowserRunDomains(r.AllowedDomains); err != nil {
		return err
	}
	if r.ViewScope != BrowserViewOwner && r.ViewScope != BrowserViewRoom {
		return fmt.Errorf("browser run decision view_scope is not recognized")
	}
	if r.MaxSteps < browserMinSteps || r.MaxSteps > browserMaxSteps {
		return fmt.Errorf("browser run decision max_steps is out of range")
	}
	if r.MaxRuntimeS < browserMinRuntimeS || r.MaxRuntimeS > browserMaxRuntimeS {
		return fmt.Errorf("browser run decision max_runtime_s is out of range")
	}
	return nil
}

// BrowserRunClaims are the non-secret values the payload digest binds beyond the
// Decision's own fields. Control returns them beside the token. The gateway forwards
// them as claims_json, and the verifier recomputes the digest from them, so an edited
// claim fails closed. AllowedDomains is in canonical (sorted) order.
type BrowserRunClaims struct {
	ThreadID       string   `json:"thread_id"`
	GoalDigest     string   `json:"goal_digest"`
	AllowedDomains []string `json:"allowed_domains"`
	ProfileScope   string   `json:"profile_scope"`
	ViewScope      string   `json:"view_scope"`
	MaxSteps       int      `json:"max_steps"`
	MaxRuntimeS    int      `json:"max_runtime_s"`
}

// validateBrowserRunDomains is the structural fail-fast on a room run's domain
// policy: 1 to 8 distinct hostnames. Quarry's SSRF and DNS guards remain the
// enforcement; this only keeps a malformed policy from being signed.
func validateBrowserRunDomains(domains []string) error {
	if len(domains) < 1 || len(domains) > browserMaxHosts {
		return fmt.Errorf("browser run allowed_domains must name 1 to %d hosts", browserMaxHosts)
	}
	seen := make(map[string]struct{}, len(domains))
	for _, host := range domains {
		if !validBrowserHost(host) {
			return fmt.Errorf("browser run allowed domain is not a permitted host")
		}
		if _, dup := seen[host]; dup {
			return fmt.Errorf("browser run allowed_domains must not repeat a host")
		}
		seen[host] = struct{}{}
	}
	return nil
}

// validBrowserHost accepts a lowercase, dot-separated hostname of at least two labels.
// Every label is letters, digits and hyphens without a leading or trailing hyphen, so
// a scheme, path, port, userinfo, wildcard or whitespace is refused. The final label
// must hold a letter: an all-digit final label is an IPv4 shape to some resolvers
// ("1.2.3", "0x7f.1"). IP literals, localhost and its subdomains are refused by name.
func validBrowserHost(host string) bool {
	if host == "" || len(host) > browserMaxHostBytes || host != strings.ToLower(host) {
		return false
	}
	if net.ParseIP(host) != nil || host == "localhost" || strings.HasSuffix(host, ".localhost") {
		return false
	}
	labels := strings.Split(host, ".")
	if len(labels) < 2 {
		return false
	}
	for _, label := range labels {
		if label == "" || len(label) > browserMaxLabelBytes || label[0] == '-' || label[len(label)-1] == '-' {
			return false
		}
		for i := 0; i < len(label); i++ {
			c := label[i]
			if (c < 'a' || c > 'z') && (c < '0' || c > '9') && c != '-' {
				return false
			}
		}
	}
	return strings.ContainsAny(labels[len(labels)-1], "abcdefghijklmnopqrstuvwxyz")
}

// canonicalBrowserDomains returns a sorted copy, so the digest and the claims do not
// depend on the order the caller listed the hosts in.
func canonicalBrowserDomains(domains []string) []string {
	out := append([]string(nil), domains...)
	sort.Strings(out)
	return out
}

// validBrowserGoalDigest accepts "sha256:" followed by 64 lowercase hex digits.
func validBrowserGoalDigest(value string) bool {
	const prefix = "sha256:"
	if len(value) != len(prefix)+sha256.Size*2 || !strings.HasPrefix(value, prefix) {
		return false
	}
	for i := len(prefix); i < len(value); i++ {
		c := value[i]
		if (c < '0' || c > '9') && (c < 'a' || c > 'f') {
			return false
		}
	}
	return true
}

// browserTokenOK accepts an empty value or a bounded token: at most limit bytes, valid
// UTF-8, no NUL byte and no surrounding whitespace.
func browserTokenOK(value string, limit int) bool {
	return len(value) <= limit && utf8.ValidString(value) &&
		!strings.ContainsRune(value, 0) && strings.TrimSpace(value) == value
}

// ValidateForBrowserRun authorizes a room browser run by the current member the
// evidence describes. The kinds are personal, room, project and case: a direct message
// or a group direct message has no browser run (D-8, as for the canvas). The role
// floor is editor, because starting a run that acts for the room is more than
// viewing it. browser_entitled must be on, and Space zero data retention refuses the
// run.
func (e PersonalThreadDecisionEvidence) ValidateForBrowserRun() error {
	if err := e.Membership.Validate(); err != nil {
		return err
	}
	switch e.Membership.Kind {
	case KindPersonal:
		if err := e.validatePersonalAuthority(); err != nil {
			return err
		}
	case KindRoom, KindProject, KindCase:
		if strings.TrimSpace(e.RecipientAudienceRef) == "" || strings.TrimSpace(e.RecipientAudienceHash) == "" {
			return authorityIncompleteDenial("shared Space recipient audience is required")
		}
		if strings.TrimSpace(e.ResourceAuthorizationRef) == "" {
			return authorityIncompleteDenial("shared Space browser run authority is incomplete")
		}
		if err := e.Privacy.Validate(); err != nil {
			return err
		}
	default:
		return kindUnsupportedDenial(string(e.Membership.Kind), "a browser run is available in personal, room, project and case Spaces only")
	}
	if !matchesOneOf(e.Membership.Role, "editor", "manager", "owner") {
		return roleBelowFloorDenial(fmt.Sprintf("space role %q cannot start a browser run", e.Membership.Role), "editor", "manager", "owner")
	}
	if !e.BrowserEntitled {
		return entitlementOffDenial("browser", "browser run authority is not active for this organization")
	}
	if e.Privacy.ZeroDataRetention {
		return zdrForbidsDenial("browser_run", "a browser run is not available under zero data retention")
	}
	return nil
}

// browserRunPayloadDigest binds the exact run the verifier must start: the goal
// commitment, the canonical domain policy, the profile, the view scope and the limits,
// under the same authority and audience the other decisions bind. The domain list is
// joined in canonical order. That join is unambiguous because validated hosts contain
// no newline. The golden vectors in testdata pin this formula for the model-gateway
// verifier (WS20-S2); this function is the source of truth.
//
// The recipient audience, privacy policy and resource authorization refs and the
// idempotency key are hashed trimmed, exactly as newEvidenceDecision writes them into
// the decision. The verifier recomputes the digest from the signed decision fields, so
// hashing the raw value would fail closed for a value with surrounding whitespace.
func browserRunPayloadDigest(evidence PersonalThreadDecisionEvidence, request BrowserRunDecisionRequest, claims BrowserRunClaims) string {
	hash := sha256.New()
	hash.Write([]byte(browserRunDigestHeader))
	for _, field := range []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID},
		{"user_id", evidence.Membership.SubjectID},
		{"space_id", evidence.Membership.SpaceRef},
		{"thread_id", claims.ThreadID},
		{"goal_digest", claims.GoalDigest},
		{"allowed_domains", strings.Join(claims.AllowedDomains, "\n")},
		{"profile_scope", claims.ProfileScope},
		{"view_scope", claims.ViewScope},
		{"max_steps", strconv.Itoa(claims.MaxSteps)},
		{"max_runtime_s", strconv.Itoa(claims.MaxRuntimeS)},
		{"recipient_audience_ref", strings.TrimSpace(evidence.RecipientAudienceRef)},
		{"recipient_audience_hash", strings.TrimSpace(evidence.RecipientAudienceHash)},
		{"privacy_policy_ref", strings.TrimSpace(evidence.Privacy.PolicyRef)},
		{"resource_authorization_ref", strings.TrimSpace(evidence.ResourceAuthorizationRef)},
		{"action_schema_hash", browserRunSchema},
		{"idempotency_key", strings.TrimSpace(request.IdempotencyKey)},
	} {
		hash.Write([]byte(field.name))
		hash.Write([]byte{0})
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(field.value)))
		hash.Write(length[:])
		hash.Write([]byte(field.value))
	}
	writeDigestRevisions(hash,
		digestRevision{"authority_revision", evidence.Membership.Revisions.Authority},
		digestRevision{"membership_revision", evidence.Membership.Revisions.Membership},
		digestRevision{"privacy_revision", evidence.Membership.Revisions.Privacy},
		digestRevision{"recipient_audience_revision", evidence.Membership.Revisions.RecipientAudience},
		digestRevision{"entitlement_revision", evidence.Membership.Revisions.Entitlement},
	)
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}

// IssueBrowserRunDecision authorizes one room browser run and returns the signed
// decision with the claims the verifier needs. A personal Space has one member and no
// room to show, so a requested room view is forced to owner there rather than refused,
// which is the narrower result. The room permission is added only for a shared Space
// whose view scope is room.
func IssueBrowserRunDecision(evidence PersonalThreadDecisionEvidence, request BrowserRunDecisionRequest, now time.Time) (Decision, BrowserRunClaims, error) {
	if err := request.Validate(); err != nil {
		return Decision{}, BrowserRunClaims{}, err
	}
	if now.IsZero() {
		return Decision{}, BrowserRunClaims{}, fmt.Errorf("browser run decision issuance time is required")
	}
	if err := evidence.ValidateForBrowserRun(); err != nil {
		return Decision{}, BrowserRunClaims{}, err
	}
	viewScope := request.ViewScope
	permissions := []string{browserPermissionRun}
	if evidence.Membership.Kind == KindPersonal {
		viewScope = BrowserViewOwner
	} else if viewScope == BrowserViewRoom {
		permissions = append(permissions, browserPermissionViewRoom)
	}
	claims := BrowserRunClaims{
		ThreadID:       request.ThreadID,
		GoalDigest:     request.GoalDigest,
		AllowedDomains: canonicalBrowserDomains(request.AllowedDomains),
		ProfileScope:   BrowserProfileEphemeral,
		ViewScope:      viewScope,
		MaxSteps:       request.MaxSteps,
		MaxRuntimeS:    request.MaxRuntimeS,
	}
	decision := newEvidenceDecision(
		evidence, request.DecisionRef, browserRunAudience, browserRunAction, browserRunSchema,
		browserRunPayloadDigest(evidence, request, claims), request.IdempotencyKey, request.Nonce,
		permissions, evidence.Privacy.ZeroDataRetention, now,
	)
	return decision, claims, nil
}
