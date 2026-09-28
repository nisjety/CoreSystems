package mailtask

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// ResolvedAlias is what user-core's internal resolve endpoint returns for a
// live alias.
type ResolvedAlias struct {
	OrgID         string
	UserID        string
	VerifiedEmail string
	// ThreadDecision is the signed mail-task-v1 Control decision user-core
	// mints from this SAME (org_id, user_id) at resolve time (delegated-
	// identity design, Gap B §2.2/§2.3). Pure pass-through from here on:
	// this resolver never inspects or verifies it -- session-core's
	// PrepareUserOwnedThread does. Empty when the alias resolves but the
	// org is ZDR, not thread-create-entitled, or user-core's signer is not
	// configured -- all additive, fail-closed omissions on user-core's
	// side, not an error here.
	ThreadDecision string
}

// AliasResolver turns an alias token into the (org_id, user_id,
// verified_email) it maps to. ok=false covers "not found", "revoked", and
// "owner has no verified email" identically -- user-core's resolve endpoint
// deliberately returns the same 404 for all three (spec §6, alias
// enumeration), and this interface preserves that: callers get no signal to
// distinguish them either.
type AliasResolver interface {
	// providerMessageID is forwarded as an additional query parameter so
	// user-core can mint a mail-task-v1 Control decision bound to this
	// exact inbound message (delegated-identity design, Gap B §2.2) --
	// never trusted from anything sender-controlled, only from this
	// already-parsed Postmark payload field.
	Resolve(ctx context.Context, alias, providerMessageID string) (resolved ResolvedAlias, ok bool, err error)
}

// HTTPAliasResolver calls user-core's GET /api/v1/internal/mail-task/resolve,
// authenticated the same way every other internal-to-internal call in this
// codebase authenticates to user-core: static X-Service-Id/X-Service-Token
// headers checked against USER_CORE_SERVICE_CREDENTIALS
// (user-core/internal/http/service_auth.go), scoped to "mail-task:resolve"
// only -- this credential cannot read or write anything else in user-core.
type HTTPAliasResolver struct {
	BaseURL      string
	ServiceID    string
	ServiceToken string
	Client       *http.Client
}

func (r *HTTPAliasResolver) Resolve(ctx context.Context, alias, providerMessageID string) (ResolvedAlias, bool, error) {
	if r == nil || strings.TrimSpace(r.BaseURL) == "" {
		return ResolvedAlias{}, false, nil
	}
	client := r.Client
	if client == nil {
		client = &http.Client{Timeout: 5 * time.Second}
	}
	endpoint := strings.TrimRight(r.BaseURL, "/") + "/api/v1/internal/mail-task/resolve?alias=" + url.QueryEscape(alias)
	if providerMessageID = strings.TrimSpace(providerMessageID); providerMessageID != "" {
		endpoint += "&provider_message_id=" + url.QueryEscape(providerMessageID)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return ResolvedAlias{}, false, err
	}
	req.Header.Set("X-Service-Id", strings.TrimSpace(r.ServiceID))
	req.Header.Set("X-Service-Token", strings.TrimSpace(r.ServiceToken))
	resp, err := client.Do(req)
	if err != nil {
		return ResolvedAlias{}, false, err
	}
	defer resp.Body.Close()
	if resp.StatusCode == http.StatusNotFound {
		// The ONLY status that means "genuinely no such alias" (user-core
		// collapses not-found/revoked/unverified-owner into this one code,
		// spec §6). Reported as (false, nil) so the caller treats it as the
		// silent-drop case, never a retry.
		_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 2048))
		return ResolvedAlias{}, false, nil
	}
	if resp.StatusCode != http.StatusOK {
		// Any OTHER status (500, 503, 403, a proxy error page, ...) means the
		// resolver itself is malfunctioning, not that the alias doesn't
		// exist. This must be an error, not a silent (false, nil): handler.go
		// only retries a delivery when Resolve returns a non-nil error, and a
		// resolver outage is exactly the kind of transient condition
		// at-least-once webhook redelivery (plus this pipeline's own
		// replay/idempotency keys) exists to paper over. Collapsing it into
		// "not found" used to make a brief user-core outage during Postmark's
		// delivery window silently and permanently drop the email (a 200
		// response Postmark never retries) instead of a retryable one.
		_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 2048))
		return ResolvedAlias{}, false, fmt.Errorf("mail task alias resolver returned HTTP %d", resp.StatusCode)
	}
	var decoded struct {
		OrgID          string `json:"org_id"`
		UserID         string `json:"user_id"`
		VerifiedEmail  string `json:"verified_email"`
		ThreadDecision string `json:"thread_decision"`
	}
	if err := json.NewDecoder(io.LimitReader(resp.Body, 1<<16)).Decode(&decoded); err != nil {
		return ResolvedAlias{}, false, err
	}
	if decoded.OrgID == "" || decoded.UserID == "" || decoded.VerifiedEmail == "" {
		return ResolvedAlias{}, false, nil
	}
	return ResolvedAlias{
		OrgID:          decoded.OrgID,
		UserID:         decoded.UserID,
		VerifiedEmail:  decoded.VerifiedEmail,
		ThreadDecision: decoded.ThreadDecision,
	}, true, nil
}
