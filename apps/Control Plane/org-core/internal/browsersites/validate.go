package browsersites

import (
	"fmt"
	"sort"
	"strings"
	"time"
)

// Error codes a validation failure can carry. They are stable: the admin and
// member views map them to text. CodeInvalidDomainPolicy is the golden's own
// error code (browser-domain-canonical.json), so a host refused here reads the
// same as one refused by the broker's normaliser.
const (
	CodeInvalidDomainPolicy      = "invalid_domain_policy"
	CodeExpectedRevisionRequired = "expected_revision_required"
	CodeInvalidMode              = "invalid_mode"
	CodeInvalidRule              = "invalid_rule"
	CodeTooManyRules             = "too_many_rules"
	CodeDuplicateHost            = "duplicate_host"
	CodeNoteTooLong              = "note_too_long"
	CodeInvalidLevel             = "invalid_level"
	CodeExpiryOutOfRange         = "expires_at_out_of_range"
)

// FieldError is one problem with a request. Path is a JSON path such as
// "rules[2].host".
type FieldError struct {
	Path    string `json:"path"`
	Code    string `json:"code"`
	Message string `json:"message"`
}

// ValidationError carries every problem found, so an admin sees them all at once.
type ValidationError struct {
	Errors []FieldError
}

func (e *ValidationError) Error() string {
	if len(e.Errors) == 0 {
		return "invalid browser site policy"
	}
	return fmt.Sprintf("invalid browser site policy: %s: %s", e.Errors[0].Path, e.Errors[0].Message)
}

// CanonicalHost returns the one spelling a host is stored and compared under, or a
// ValidationError. It follows browser-domain-canonical.json: surrounding space is
// trimmed and letters are folded to lower case; the result is ASCII labels of
// letters, digits and hyphens, at least two labels, no hyphen at a label edge, a
// label of at most 63 octets and a host of at most 253. IP literals are refused,
// and so is a last label of digits, which is how a dotted IPv4 address reads. A
// host with a port, path, scheme, user, wildcard or dot at an end is refused too.
func CanonicalHost(raw string) (string, error) {
	host := strings.ToLower(strings.TrimSpace(raw))
	if !validHost(host) {
		return "", domainError(raw)
	}
	return host, nil
}

// CanonicalHostList is the golden list rule: one to MaxResolveHosts raw entries,
// all-or-nothing, duplicates collapsed after folding, returned in bytewise order.
// The cap counts raw entries, so the same host thirty-three times is refused.
func CanonicalHostList(raw []string) ([]string, error) {
	if len(raw) == 0 || len(raw) > MaxResolveHosts {
		return nil, &ValidationError{Errors: []FieldError{{
			Path:    "hosts",
			Code:    CodeInvalidDomainPolicy,
			Message: fmt.Sprintf("between 1 and %d hosts are required", MaxResolveHosts),
		}}}
	}
	seen := make(map[string]struct{}, len(raw))
	hosts := make([]string, 0, len(raw))
	for _, entry := range raw {
		host, err := CanonicalHost(entry)
		if err != nil {
			return nil, err
		}
		if _, dup := seen[host]; dup {
			continue
		}
		seen[host] = struct{}{}
		hosts = append(hosts, host)
	}
	sort.Strings(hosts)
	return hosts, nil
}

func domainError(raw string) error {
	return &ValidationError{Errors: []FieldError{{
		Path:    "host",
		Code:    CodeInvalidDomainPolicy,
		Message: fmt.Sprintf("%q is not a host name the policy can hold", strings.TrimSpace(raw)),
	}}}
}

func validHost(host string) bool {
	if host == "" || len(host) > 253 || !strings.Contains(host, ".") {
		return false
	}
	labels := strings.Split(host, ".")
	for _, label := range labels {
		if len(label) == 0 || len(label) > 63 {
			return false
		}
		if label[0] == '-' || label[len(label)-1] == '-' {
			return false
		}
		for i := 0; i < len(label); i++ {
			c := label[i]
			if !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '-') {
				return false
			}
		}
	}
	return !allDigits(labels[len(labels)-1])
}

func allDigits(label string) bool {
	for i := 0; i < len(label); i++ {
		if label[i] < '0' || label[i] > '9' {
			return false
		}
	}
	return true
}

// ValidatePut checks a whole policy write and returns it normalised: rules sorted
// by host, hosts canonical, notes trimmed. It writes nothing; the service calls it
// before the store.
func ValidatePut(req PutRequest) (PutRequest, error) {
	var v validator
	if req.ExpectedRevision == nil || *req.ExpectedRevision < 0 {
		v.add("expected_revision", CodeExpectedRevisionRequired, "expected_revision is required and must not be negative")
	}
	mode := strings.TrimSpace(req.Mode)
	if mode != ModeOpen && mode != ModeAllowlist {
		v.add("mode", CodeInvalidMode, "mode must be open or allowlist")
	}
	if len(req.Rules) > MaxRules {
		v.add("rules", CodeTooManyRules, fmt.Sprintf("at most %d rules", MaxRules))
	}
	rules := make([]Rule, 0, len(req.Rules))
	seen := make(map[string]struct{}, len(req.Rules))
	for i, rule := range req.Rules {
		path := fmt.Sprintf("rules[%d]", i)
		host, err := CanonicalHost(rule.Host)
		if err != nil {
			v.add(path+".host", CodeInvalidDomainPolicy, "host is not a name the policy can hold")
			continue
		}
		if _, dup := seen[host]; dup {
			v.add(path+".host", CodeDuplicateHost, "each host may carry one rule")
			continue
		}
		seen[host] = struct{}{}
		value := strings.TrimSpace(rule.Rule)
		if value != RuleBlock && value != RuleRead && value != RuleAct {
			v.add(path+".rule", CodeInvalidRule, "rule must be block, read or act")
		}
		note := strings.TrimSpace(rule.Note)
		if len(note) > MaxNoteLength {
			v.add(path+".note", CodeNoteTooLong, fmt.Sprintf("note is at most %d characters", MaxNoteLength))
		}
		rules = append(rules, Rule{Host: host, Rule: value, Note: note})
	}
	if err := v.err(); err != nil {
		return PutRequest{}, err
	}
	sort.Slice(rules, func(i, j int) bool { return rules[i].Host < rules[j].Host })
	revision := *req.ExpectedRevision
	return PutRequest{ExpectedRevision: &revision, Mode: mode, Rules: rules}, nil
}

// PrefInput is a member's choice after validation: the level and the instant it
// stops applying.
type PrefInput struct {
	Level     string
	ExpiresAt time.Time
}

// ValidatePref checks one member's choice. An absent expires_at means the default
// lifetime from now; a given one must lie after now and within MaxPrefLifetime.
func ValidatePref(req PrefRequest, now time.Time) (PrefInput, error) {
	var v validator
	if req.ExpectedRevision == nil || *req.ExpectedRevision < 0 {
		v.add("expected_revision", CodeExpectedRevisionRequired, "expected_revision is required and must not be negative")
	}
	level := strings.TrimSpace(req.Level)
	if level != LevelNone && level != LevelRead && level != LevelAct {
		v.add("level", CodeInvalidLevel, "level must be none, read or act")
	}
	expires := now.Add(MaxPrefLifetime)
	if req.ExpiresAt != nil {
		expires = req.ExpiresAt.UTC()
		if !expires.After(now) || expires.After(now.Add(MaxPrefLifetime)) {
			v.add("expires_at", CodeExpiryOutOfRange, "expires_at must be later than now and at most 90 days away")
		}
	}
	if err := v.err(); err != nil {
		return PrefInput{}, err
	}
	return PrefInput{Level: level, ExpiresAt: expires}, nil
}

type validator struct {
	errors []FieldError
}

func (v *validator) add(path, code, message string) {
	v.errors = append(v.errors, FieldError{Path: path, Code: code, Message: message})
}

func (v *validator) err() error {
	if len(v.errors) == 0 {
		return nil
	}
	return &ValidationError{Errors: v.errors}
}
