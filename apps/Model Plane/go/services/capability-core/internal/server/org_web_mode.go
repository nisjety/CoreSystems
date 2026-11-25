package server

import (
	"context"
	"errors"
	"log/slog"
	"sync"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// The org's web.mode gate (F-f09-13). An org chooses off, cached_only or live for public web access, and whether
// its browser tools follow that choice too (web.covers_browser). execution-core applies web.mode on the RunAgent
// path only: the durable ExecuteStep path carries no org policy, and the browser tools sit in no web.mode set in
// either path. This gate sits where every call lands, EvaluatePolicy, and it names the capability, never a tool, so
// it does not depend on which tool a call names. It only tightens, and it reads no content.

// orgWebPublicCapabilities are the public-web capabilities web.mode withholds: the capabilities behind the tools
// execution-core's ORG_WEB_TOOLS names, that is web search and fetch and the crawl, map and batch tools.
var orgWebPublicCapabilities = map[string]bool{
	"cap.tool.http": true,
	"cap.web.crawl": true,
	"cap.web.map":   true,
	"cap.web.batch": true,
}

// orgWebBrowserCapabilities are the browser capabilities. The browser switch withholds them under web.mode, for an
// org whose web.covers_browser does not opt out.
var orgWebBrowserCapabilities = map[string]bool{
	"cap.browser.open":    true,
	"cap.browser.observe": true,
	"cap.browser.act":     true,
}

const (
	// orgWebModeLive is the one web.mode that leaves public web access open. Any other value, one this build does
	// not know included, withholds it.
	orgWebModeLive = "live"
	// orgWebCoversBrowserOptOut is the web.covers_browser value (a bool, O-19) that leaves the browser capabilities open
	// under web.mode. Any other value, a JSON null and a missing key included, covers them: an unreadable choice fails
	// closed to covers.
	orgWebCoversBrowserOptOut = "false"
	// orgWebModeCacheTTL is how long one org's web keys are reused. A change in the policy reaches the gate within it.
	orgWebModeCacheTTL = 5 * time.Second

	orgWebOffReason         = "org_web_off"
	orgWebUnavailableReason = "org_web_unavailable"
)

// orgWebModeReader reads the two keys of an org's policy document that the gate needs. mode is the org's web.mode
// as stored; coversBrowser is false only for the opt-out value of web.covers_browser. A failed read is returned as
// an error, never guessed at.
type orgWebModeReader interface {
	WebMode(ctx context.Context, orgID string) (mode string, coversBrowser bool, err error)
}

// orgWebModeDB is the query surface the store needs. *pgxpool.Pool satisfies it.
type orgWebModeDB interface {
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
}

// orgWebModeReadSQL reads the two keys from the org's policy document, which is the settings map itself
// (org_agent_policy's doc, api/org_agent_policy.go). For each key it returns whether the key is present and its
// text: a present key with a JSON null has no text, and the reader treats that as a value it cannot read. The
// covers key has text only when it is a JSON boolean (O-19), so a stored string "false" is not an opt-out.
const orgWebModeReadSQL = `
SELECT doc ? 'web.mode', doc->>'web.mode', doc ? 'web.covers_browser',
       CASE WHEN jsonb_typeof(doc->'web.covers_browser') = 'boolean' THEN doc->>'web.covers_browser' END
FROM org_agent_policy
WHERE org_id = $1
`

// OrgWebModeStore is the pgx reader of an org's web keys. The gate reads it on every call to a web capability, so
// each org's answer is kept for orgWebModeCacheTTL. A failed read is never kept.
type OrgWebModeStore struct {
	db    orgWebModeDB
	clock func() time.Time

	mu    sync.Mutex
	cache map[string]orgWebModeEntry
}

// orgWebModeEntry is one org's answer and the time it stops being reused.
type orgWebModeEntry struct {
	mode          string
	coversBrowser bool
	expires       time.Time
}

// NewOrgWebModeStore reads the org's web keys through pool.
func NewOrgWebModeStore(pool *pgxpool.Pool) *OrgWebModeStore {
	return newOrgWebModeStore(pool, time.Now)
}

func newOrgWebModeStore(db orgWebModeDB, clock func() time.Time) *OrgWebModeStore {
	return &OrgWebModeStore{db: db, clock: clock, cache: make(map[string]orgWebModeEntry)}
}

// WebMode implements orgWebModeReader. A cached answer is returned while it is fresh; otherwise the database is read
// and the answer is cached for orgWebModeCacheTTL. Entries that have expired are dropped on each write, so the map
// holds only the orgs read within the last window.
func (s *OrgWebModeStore) WebMode(ctx context.Context, orgID string) (string, bool, error) {
	now := s.clock()
	s.mu.Lock()
	entry, cached := s.cache[orgID]
	s.mu.Unlock()
	if cached && now.Before(entry.expires) {
		return entry.mode, entry.coversBrowser, nil
	}
	mode, coversBrowser, err := s.read(ctx, orgID)
	if err != nil {
		return "", false, err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	for key, stale := range s.cache {
		if !now.Before(stale.expires) {
			delete(s.cache, key)
		}
	}
	s.cache[orgID] = orgWebModeEntry{mode: mode, coversBrowser: coversBrowser, expires: now.Add(orgWebModeCacheTTL)}
	return mode, coversBrowser, nil
}

// read asks the database for the org's two keys. An org with no row, and a key that is absent, read as the schema
// default: live, and web.covers_browser covering the browser tools. A key that is present is read as written: a JSON
// null, which has no text, denies the web, and web.covers_browser covers the browser tools unless it is exactly the
// opt-out.
func (s *OrgWebModeStore) read(ctx context.Context, orgID string) (string, bool, error) {
	var (
		hasMode, hasCovers bool
		mode, covers       *string
	)
	err := s.db.QueryRow(ctx, orgWebModeReadSQL, orgID).Scan(&hasMode, &mode, &hasCovers, &covers)
	if errors.Is(err, pgx.ErrNoRows) {
		return orgWebModeLive, true, nil
	}
	if err != nil {
		return "", false, err
	}
	modeWord := orgWebModeLive
	if hasMode {
		modeWord = ""
		if mode != nil {
			modeWord = *mode
		}
	}
	coversBrowser := true
	if hasCovers && covers != nil && *covers == orgWebCoversBrowserOptOut {
		coversBrowser = false
	}
	return modeWord, coversBrowser, nil
}

// applyOrgWebMode withholds a web capability from a call whose org turned web access off (F-f09-13). It only
// tightens: a decision that is already a deny stands, and a capability outside the gated sets is returned as it
// was, without a read. The public-web set is gated by web.mode alone. The browser set is gated only while the
// browser switch is on and the org's web.covers_browser does not opt out. When the org's keys cannot be read, the
// gated capabilities are withheld with org_web_unavailable, because the org's choice cannot be known.
func (s *Server) applyOrgWebMode(ctx context.Context, orgID string, req *mpv1.EvaluatePolicyRequest, result *policy.Result) *policy.Result {
	if !s.orgWebModeGate || result.Decision == policy.DecisionDeny {
		return result
	}
	capabilityID := req.GetCapabilityId()
	public := orgWebPublicCapabilities[capabilityID]
	browser := s.orgWebModeBrowser && orgWebBrowserCapabilities[capabilityID]
	if !public && !browser {
		return result
	}
	mode, coversBrowser, err := s.orgWebModes.WebMode(ctx, orgID)
	if err != nil {
		slog.Error("org web mode unreadable; the web capability is withheld", "org_id", orgID, "reason", orgWebUnavailableReason, "error", err)
		return orgWebDeny(orgWebUnavailableReason, result.ToolPolicyChecked)
	}
	if !public && !coversBrowser {
		// A browser capability whose org opted out of browser coverage keeps the decision it had.
		return result
	}
	if mode == orgWebModeLive {
		return result
	}
	return orgWebDeny(orgWebOffReason, result.ToolPolicyChecked)
}

// orgWebDeny is the decision of a web capability the org's web keys withhold. It names the gate and the reason
// code, and carries no content.
func orgWebDeny(reason string, toolPolicyChecked bool) *policy.Result {
	return &policy.Result{
		Decision:          policy.DecisionDeny,
		Reason:            reason,
		PolicySource:      policy.PolicySourceOrgWebMode,
		ToolPolicyChecked: toolPolicyChecked,
	}
}
