package modelentitlement

import (
	"fmt"
	"regexp"
	"sort"
	"strings"
)

// Limits a policy set must stay inside (WS10-S2). The body limit is enforced by
// the HTTP handler; the rest are enforced here.
const (
	MaxEntries     = 500
	MaxEntryLength = 128
	MaxRoles       = 20
	MaxSurfaces    = 16
	MaxBodyBytes   = 64 << 10
)

// Error codes a validation failure can carry. They are stable: the admin UI maps
// them to text.
const (
	CodeExpectedRevisionRequired = "expected_revision_required"
	CodeInvalidAllowMode         = "invalid_allow_mode"
	CodeAllowRequiresListedMode  = "allow_requires_listed_mode"
	CodeEmptyEntry               = "empty_entry"
	CodeEntryTooLong             = "entry_too_long"
	CodeGlobNotAllowed           = "glob_not_allowed"
	CodeInvalidEntry             = "invalid_entry"
	CodeIntentIDNotAllowed       = "intent_id_not_allowed"
	CodeTooManyEntries           = "too_many_entries"
	CodeUnknownEffort            = "unknown_effort"
	CodeUnknownPrivacyTier       = "unknown_privacy_tier"
	CodeInvalidSurface           = "invalid_surface"
	CodeTooManySurfaces          = "too_many_surfaces"
	CodeEmptySurfaceDefault      = "empty_surface_default"
	CodeDefaultModelIsSelector   = "default_model_is_selector"
	CodeDefaultModelNotPermitted = "default_model_not_permitted"
	CodeDefaultEffortAboveMax    = "default_effort_above_max_effort"
	CodeInvalidRole              = "invalid_role"
	CodeDuplicateRole            = "duplicate_role"
	CodeTooManyRoles             = "too_many_roles"
)

// intentIDPrefix marks a Verevon intent mode (verevon-budget, verevon-balance,
// verevon-genius). An intent is not a model id: inference-core resolves it to a
// concrete model at admission, so an allow or deny entry naming one would
// silently never match.
const intentIDPrefix = "verevon-"

// effortRank is the reasoning-effort ladder, weakest first. It mirrors
// inference-core's provider/effort.rs Level::ALL (WS11); the test
// TestEffortLadderMatchesInferenceCore pins the two lists together.
var effortRank = map[string]int{
	"minimal": 1,
	"low":     2,
	"medium":  3,
	"high":    4,
	"xhigh":   5,
	"max":     6,
}

// privacyTiers are the tiers a policy may demand, as inference-core's
// PrivacyTier labels them. The unspecified tier is "no minimum" and is written
// as an empty string, never stored.
var privacyTiers = map[string]struct{}{
	"global":          {},
	"eu_resident":     {},
	"zdr_contractual": {},
	"sovereign":       {},
}

var (
	providerIDPattern = regexp.MustCompile(`^[a-z0-9][a-z0-9._-]*$`)
	surfacePattern    = regexp.MustCompile(`^[a-z][a-z0-9_-]{0,31}$`)
	rolePattern       = regexp.MustCompile(`^[a-z0-9][a-z0-9._-]{0,63}$`)
)

// FieldError is one problem with a request body. Path is a JSON path such as
// "roles.admin.allow[2]".
type FieldError struct {
	Path    string `json:"path"`
	Code    string `json:"code"`
	Message string `json:"message"`
}

// ValidationError carries every problem found, so an admin sees them all at
// once instead of fixing one per request.
type ValidationError struct {
	Errors []FieldError
}

func (e *ValidationError) Error() string {
	parts := make([]string, 0, len(e.Errors))
	for _, fieldErr := range e.Errors {
		parts = append(parts, fieldErr.Path+": "+fieldErr.Code)
	}
	return "invalid model policy: " + strings.Join(parts, "; ")
}

type validator struct {
	errs []FieldError
}

func (v *validator) add(path, code, message string) {
	v.errs = append(v.errs, FieldError{Path: path, Code: code, Message: message})
}

// ValidatePut checks a PUT body and returns it normalised: lower-cased,
// de-duplicated and sorted entries, so that two requests that mean the same
// policy store the same bytes. The returned request is a copy.
func ValidatePut(req PutRequest) (PutRequest, error) {
	v := &validator{}
	if req.ExpectedRevision == nil || *req.ExpectedRevision < 0 {
		v.add("expected_revision", CodeExpectedRevisionRequired, "expected_revision is required and must be zero or greater (0 for the first write)")
	}

	out := PutRequest{ExpectedRevision: req.ExpectedRevision}
	if req.OrgDefault != nil {
		normalised := v.policy("org_default", *req.OrgDefault)
		out.OrgDefault = &normalised
	}

	if len(req.Roles) > MaxRoles {
		v.add("roles", CodeTooManyRoles, fmt.Sprintf("at most %d role overrides are allowed", MaxRoles))
	}
	out.Roles = make(map[string]Policy, len(req.Roles))
	names := make([]string, 0, len(req.Roles))
	for name := range req.Roles {
		names = append(names, name)
	}
	sort.Strings(names)
	for _, name := range names {
		role := NormalizeRole(name)
		path := "roles." + name
		if !rolePattern.MatchString(role) {
			v.add(path, CodeInvalidRole, "a role name is 1 to 64 characters of letters, digits, '.', '_' or '-'")
			continue
		}
		if _, duplicate := out.Roles[role]; duplicate {
			v.add(path, CodeDuplicateRole, "two role names are the same once lower-cased")
			continue
		}
		out.Roles[role] = v.policy("roles."+role, req.Roles[name])
	}

	if len(v.errs) > 0 {
		return PutRequest{}, &ValidationError{Errors: v.errs}
	}
	return out, nil
}

// NormalizeRole is the one place a role name is folded: the stored key and the
// member's role are compared lower-cased and trimmed.
func NormalizeRole(role string) string {
	return strings.ToLower(strings.TrimSpace(role))
}

func (v *validator) policy(path string, in Policy) Policy {
	out := Policy{
		AllowMode:      strings.ToLower(strings.TrimSpace(in.AllowMode)),
		MaxEffort:      strings.ToLower(strings.TrimSpace(in.MaxEffort)),
		MinPrivacyTier: strings.ToLower(strings.TrimSpace(in.MinPrivacyTier)),
	}
	if out.AllowMode != AllowModeAll && out.AllowMode != AllowModeListed {
		v.add(path+".allow_mode", CodeInvalidAllowMode, "allow_mode must be \"all\" or \"listed\"")
	}
	out.Allow = v.entries(path+".allow", in.Allow)
	out.Deny = v.entries(path+".deny", in.Deny)
	if out.AllowMode == AllowModeAll && len(out.Allow) > 0 {
		v.add(path+".allow", CodeAllowRequiresListedMode, "an allow list means nothing in \"all\" mode; use \"listed\" or remove the list")
	}

	if out.MaxEffort != "" {
		if _, known := effortRank[out.MaxEffort]; !known {
			v.add(path+".max_effort", CodeUnknownEffort, "max_effort is not a rung of the effort ladder")
		}
	}
	if out.MinPrivacyTier != "" {
		if _, known := privacyTiers[out.MinPrivacyTier]; !known {
			v.add(path+".min_privacy_tier", CodeUnknownPrivacyTier, "min_privacy_tier is not a known privacy tier")
		}
	}

	out.Defaults = v.defaults(path+".defaults", in.Defaults, out)
	return out
}

// entries lower-cases, de-duplicates and sorts a list, and reports every entry
// that is not an exact model id or a provider selector. It also refuses an intent
// mode: the policy judges the concrete model an intent resolves to, so an entry
// naming the intent would never match. defaults are checked separately, so a
// surface may still name an intent.
func (v *validator) entries(path string, in []string) []string {
	if len(in) > MaxEntries {
		v.add(path, CodeTooManyEntries, fmt.Sprintf("at most %d entries are allowed", MaxEntries))
		return []string{}
	}
	seen := make(map[string]struct{}, len(in))
	out := make([]string, 0, len(in))
	for index, raw := range in {
		entry, code, message := normalizeEntry(raw)
		if code == "" && strings.HasPrefix(entry, intentIDPrefix) {
			code = CodeIntentIDNotAllowed
			message = "an intent mode is not a model: the policy judges the concrete model it resolves to, so list or deny that model instead"
		}
		if code != "" {
			v.add(fmt.Sprintf("%s[%d]", path, index), code, message)
			continue
		}
		if _, duplicate := seen[entry]; duplicate {
			continue
		}
		seen[entry] = struct{}{}
		out = append(out, entry)
	}
	sort.Strings(out)
	return out
}

// normalizeEntry accepts an exact model id or "provider:<id>". There are no
// globs: an allow pattern would widen a policy silently when a model is added,
// so a wildcard is refused rather than treated literally.
func normalizeEntry(raw string) (entry, code, message string) {
	entry = strings.ToLower(strings.TrimSpace(raw))
	switch {
	case entry == "":
		return "", CodeEmptyEntry, "an entry cannot be empty"
	case len(entry) > MaxEntryLength:
		return "", CodeEntryTooLong, fmt.Sprintf("an entry is at most %d characters", MaxEntryLength)
	case strings.ContainsAny(entry, "*?[]{}"):
		return "", CodeGlobNotAllowed, "entries are exact model ids or provider:<id>; patterns are not supported"
	}
	if strings.HasPrefix(entry, ProviderPrefix) {
		if !providerIDPattern.MatchString(strings.TrimPrefix(entry, ProviderPrefix)) {
			return "", CodeInvalidEntry, "a provider selector is provider:<id> with a plain provider id"
		}
		return entry, "", ""
	}
	for index, character := range entry {
		alphanumeric := (character >= 'a' && character <= 'z') || (character >= '0' && character <= '9')
		if alphanumeric || (index > 0 && strings.ContainsRune("._:/@+-", character)) {
			continue
		}
		return "", CodeInvalidEntry, "a model id is letters, digits and ._:/@+- and starts with a letter or digit"
	}
	return entry, "", ""
}

// defaults validates the per-surface defaults against the policy they belong
// to: a default model must not be one the same policy refuses, and a default
// effort must not exceed the policy's own ceiling.
func (v *validator) defaults(path string, in map[string]SurfaceDefault, policy Policy) map[string]SurfaceDefault {
	if len(in) == 0 {
		return nil
	}
	if len(in) > MaxSurfaces {
		v.add(path, CodeTooManySurfaces, fmt.Sprintf("at most %d surfaces are allowed", MaxSurfaces))
		return nil
	}
	surfaces := make([]string, 0, len(in))
	for surface := range in {
		surfaces = append(surfaces, surface)
	}
	sort.Strings(surfaces)

	out := make(map[string]SurfaceDefault, len(in))
	for _, surface := range surfaces {
		surfacePath := path + "." + surface
		if !surfacePattern.MatchString(surface) {
			v.add(surfacePath, CodeInvalidSurface, "a surface name is lower-case letters, digits, '_' or '-' and starts with a letter")
			continue
		}
		raw := in[surface]
		next := SurfaceDefault{
			Model:  strings.ToLower(strings.TrimSpace(raw.Model)),
			Effort: strings.ToLower(strings.TrimSpace(raw.Effort)),
		}
		if next.Model == "" && next.Effort == "" {
			v.add(surfacePath, CodeEmptySurfaceDefault, "a surface default sets a model, an effort or both")
			continue
		}
		if next.Model != "" {
			model, code, message := normalizeEntry(next.Model)
			switch {
			case code != "":
				v.add(surfacePath+".model", code, message)
			case strings.HasPrefix(model, ProviderPrefix):
				v.add(surfacePath+".model", CodeDefaultModelIsSelector, "a default is one model id, not a provider selector")
			case !defaultModelPermitted(policy, model):
				v.add(surfacePath+".model", CodeDefaultModelNotPermitted, "the default model is refused by the policy it belongs to")
			default:
				next.Model = model
			}
		}
		if next.Effort != "" {
			rank, known := effortRank[next.Effort]
			ceiling, capped := effortRank[policy.MaxEffort]
			switch {
			case !known:
				v.add(surfacePath+".effort", CodeUnknownEffort, "effort is not a rung of the effort ladder")
			case capped && rank > ceiling:
				v.add(surfacePath+".effort", CodeDefaultEffortAboveMax, "the default effort is above the policy's max_effort")
			}
		}
		out[surface] = next
	}
	return out
}

// defaultModelPermitted is the write-time check that a default model is not
// one its own policy refuses. org-core does not know which provider serves a
// model, so a provider selector cannot be evaluated here: when the policy lists
// only by selector the default is accepted, and inference-core re-checks it at
// admission, where the catalogue is known. An intent mode is the same case: no
// list can name one (entries refuses it), and the model it resolves to is only
// known at admission, so a default that names one is not refused here.
func defaultModelPermitted(policy Policy, model string) bool {
	if containsExact(policy.Deny, model) {
		return false
	}
	if policy.AllowMode == AllowModeAll {
		return true
	}
	if strings.HasPrefix(model, intentIDPrefix) {
		return true
	}
	if containsExact(policy.Allow, model) {
		return true
	}
	for _, entry := range policy.Allow {
		if strings.HasPrefix(entry, ProviderPrefix) {
			return true
		}
	}
	return false
}

func containsExact(list []string, value string) bool {
	for _, entry := range list {
		if entry == value {
			return true
		}
	}
	return false
}
