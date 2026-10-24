// Package browsersites is org-core's agent-browser site policy (WS21-S3, DEC-19):
// which hosts an organisation's agent browser may reach, at what level, and the
// remembered choice a member has made for a host.
//
// org-core stores, validates, resolves and explains the policy. It never enforces
// it: the browser broker pulls the resolved answer at grant mint, and Quarry stays
// the final enforcer. The policy holds hostnames, levels, revisions and actor ids
// only, so it carries no content and needs no retention class.
package browsersites

import "time"

const (
	// ModeOpen and ModeAllowlist are the two admin postures. Open is the default:
	// every host not blocked is reachable at the ceiling of ask. Allowlist reaches
	// only the hosts an admin lists with read or act.
	ModeOpen      = "open"
	ModeAllowlist = "allowlist"

	// RuleBlock, RuleRead and RuleAct are the per-host admin rules. Block wins over
	// everything. Read and act set the most a member may be granted for the host.
	RuleBlock = "block"
	RuleRead  = "read"
	RuleAct   = "act"

	// LevelNone, LevelRead and LevelAct are a member's remembered choice. None is an
	// explicit refusal the member made once and does not want asked again.
	LevelNone = "none"
	LevelRead = "read"
	LevelAct  = "act"

	// CapNone is the ceiling of a host the policy does not reach at all.
	CapNone = "none"

	// EffectiveDeny, EffectiveAsk, EffectiveRead and EffectiveAct are the answer for
	// one host. Ask means no live choice exists yet and the member must be asked.
	EffectiveDeny = "deny"
	EffectiveAsk  = "ask"
	EffectiveRead = "read"
	EffectiveAct  = "act"

	// SourceOrgBlock, SourceOrgCap, SourceUserPref and SourceDefault name what
	// decided the answer. Org cap means the admin's policy, not the member, settled
	// it (an allowlist that does not list the host, or a read ceiling over an act
	// choice); default means nothing decided it and the member is asked.
	SourceOrgBlock = "org_block"
	SourceOrgCap   = "org_cap"
	SourceUserPref = "user_pref"
	SourceDefault  = "default"

	// MaxRules caps the admin's list. MaxPrefsPerUser caps one member's remembered
	// choices in one organisation (WS21-S3). MaxResolveHosts is the raw entry cap of
	// a resolve request, the golden's list rule (browser-domain-canonical.json).
	MaxRules        = 500
	MaxPrefsPerUser = 200
	MaxResolveHosts = 32

	// MaxNoteLength bounds the free-text note on an admin rule. A note is an
	// admin's remark about a host, never page content, and it is kept short.
	MaxNoteLength = 200

	// MaxPrefLifetime is the longest a member's choice may run; the default is the
	// same 90 days the table defaults to.
	MaxPrefLifetime = 90 * 24 * time.Hour

	// MaxBodyBytes bounds one PUT body, as the model policy does.
	MaxBodyBytes = 64 << 10
)

// Rule is one admin rule on one canonical host.
type Rule struct {
	Host string `json:"host"`
	Rule string `json:"rule"`
	Note string `json:"note,omitempty"`
}

// PolicySet is everything stored for one organisation. A zero Revision means
// nothing was ever written, which reads as open mode with no rules.
type PolicySet struct {
	OrganizationID string     `json:"organization_id"`
	Revision       int64      `json:"revision"`
	Mode           string     `json:"mode"`
	Rules          []Rule     `json:"rules"`
	UpdatedBy      string     `json:"updated_by"`
	UpdatedAt      *time.Time `json:"updated_at"`
}

// PutRequest replaces the whole policy set. ExpectedRevision is a pointer so a
// body that omits it is refused instead of reading as "first write".
type PutRequest struct {
	ExpectedRevision *int64 `json:"expected_revision"`
	Mode             string `json:"mode"`
	Rules            []Rule `json:"rules"`
}

// Pref is one member's remembered choice for one host.
type Pref struct {
	Host      string    `json:"host"`
	Level     string    `json:"level"`
	ExpiresAt time.Time `json:"expires_at"`
	UpdatedAt time.Time `json:"updated_at"`
}

// PrefSet is one member's choices in one organisation, with the policy revision
// they were read against.
type PrefSet struct {
	OrganizationID string `json:"organization_id"`
	UserID         string `json:"user_id"`
	Revision       int64  `json:"revision"`
	Prefs          []Pref `json:"prefs"`
}

// PrefRequest writes one member's choice. ExpectedRevision is the policy revision
// the member decided against; a choice made against a superseded policy is refused
// so the member re-reads it. ExpiresAt is optional and defaults to 90 days.
type PrefRequest struct {
	ExpectedRevision *int64     `json:"expected_revision"`
	Level            string     `json:"level"`
	ExpiresAt        *time.Time `json:"expires_at"`
}

// Decision is the answer for one host.
type Decision struct {
	Host      string `json:"host"`
	Effective string `json:"effective"`
	Cap       string `json:"cap"`
	Source    string `json:"source"`
}

// Resolution is the answer for a set of hosts for one caller, at one revision. It
// is the body of the internal resolve route.
type Resolution struct {
	OrganizationID string     `json:"organization_id"`
	UserID         string     `json:"user_id,omitempty"`
	Revision       int64      `json:"revision"`
	Decisions      []Decision `json:"decisions"`
}

// Explanation is the answer for one host and the layers that produced it, for the
// member's own view. Nothing here is another member's data.
type Explanation struct {
	OrganizationID string     `json:"organization_id"`
	UserID         string     `json:"user_id"`
	Revision       int64      `json:"revision"`
	Mode           string     `json:"mode"`
	Decision       Decision   `json:"decision"`
	OrgRule        string     `json:"org_rule"`
	UserLevel      string     `json:"user_level"`
	UserExpiresAt  *time.Time `json:"user_expires_at"`
}
