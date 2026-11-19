// Package flagregistry is the Control Plane's typed registry of feature flags and
// config knobs. Each entry declares a flag once: its name, shape, default,
// owning plane and where it is set. The registry does not read the environment
// for services. Services keep their own parsers until they migrate. A new switch
// is read as the exact literal "true" (OnLiteral "true"); ParseSwitch is the
// canonical vocabulary for the switches that do not set OnLiteral or
// OnLiteralAnyCase.
package flagregistry

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"regexp"
	"slices"
	"strings"
	"unicode/utf8"
)

// SchemaV1 is the only document schema this package reads.
const SchemaV1 = "verevon.flag-registry/v1"

const maxPurposeRunes = 240

// Kind is the shape of a flag's value.
type Kind string

const (
	// KindSwitch is a boolean. It is read as its OnLiteral or OnLiteralAnyCase when one is
	// set, and otherwise with the canonical vocabulary (ParseSwitch).
	KindSwitch Kind = "switch"
	// KindChoice is one value from the closed set Values.
	KindChoice Kind = "choice"
	// KindText is an opaque string that the owning service validates.
	KindText Kind = "text"
)

// Plane names the plane that owns a flag. The set is closed, so a typo fails validation.
type Plane string

const (
	PlaneControl     Plane = "control-plane"
	PlaneModel       Plane = "model-plane"
	PlaneFrontend    Plane = "frontend-plane"
	PlaneData        Plane = "data-plane"
	PlaneIngestion   Plane = "ingestion-plane"
	PlaneApplication Plane = "application-plane"
)

var knownPlanes = []Plane{PlaneControl, PlaneModel, PlaneFrontend, PlaneData, PlaneIngestion, PlaneApplication}

// Source says where a flag is set.
type Source string

const (
	// SourceCompose flags are set by a compose environment block. Their
	// declarations are checked against the compose file.
	SourceCompose Source = "compose"
	// SourceCode flags are read by code. CodePath names the file that reads
	// them. Compose may set them too, and then their declarations are checked
	// like any other flag's.
	SourceCode Source = "code"
)

// Stage is how far a flag has rolled out. Dark means not offered yet, beta means
// offered to a set the owner picks, and ga means offered to everyone. The registry
// records the stage and rolls nothing out.
type Stage string

const (
	// StageDark is a flag that is not offered yet. An entry that does not say its
	// stage is dark.
	StageDark Stage = "dark"
	// StageBeta is a flag offered to a set the owner picks.
	StageBeta Stage = "beta"
	// StageGA is a flag offered to everyone.
	StageGA Stage = "ga"
)

// AgentAccess says whether a model-facing tool may read a flag's value.
type AgentAccess string

const (
	// AgentAccessNone keeps the value from every model-facing tool. An entry that
	// does not say gets none.
	AgentAccessNone AgentAccess = "none"
	// AgentAccessRead lets a model-facing tool read the value.
	AgentAccessRead AgentAccess = "read"
)

// ChoiceRead says how a choice's service compares the raw variable with the choice values.
// A choice that sets none is trimmed. Each constant names one reader the registry models.
type ChoiceRead string

const (
	// ChoiceReadExact is a reader that compares the raw variable as written. A padded or
	// cased value is not a choice, so the typed read refuses it.
	ChoiceReadExact ChoiceRead = "exact"
	// ChoiceReadAnyCase is a reader that trims surrounding space and folds ASCII case before
	// it compares, so " ENFORCE " reads as enforce. Its values are lowercase.
	ChoiceReadAnyCase ChoiceRead = "anyCase"
)

// Declaration is one compose environment entry that sets a flag. Env is the
// container variable; the flag name is the host variable it reads from.
type Declaration struct {
	Service string `json:"service"`
	Env     string `json:"env"`
	Default string `json:"default"`
}

// Flag is one registry entry. Default is the value a deployment sees when the
// variable is unset, written the way the source writes it.
type Flag struct {
	Name         string        `json:"name"`
	Kind         Kind          `json:"kind"`
	Owner        Plane         `json:"owner"`
	Source       Source        `json:"source"`
	Default      string        `json:"default"`
	Values       []string      `json:"values,omitempty"`
	Purpose      string        `json:"purpose"`
	Declarations []Declaration `json:"declarations,omitempty"`
	CodePath     string        `json:"codePath,omitempty"`
	// CodeDefault is what the code does when the variable is unset, when that
	// differs from Default. Only compose flags carry it.
	CodeDefault string `json:"codeDefault,omitempty"`
	// DriftReason says why CodeDefault and Default disagree.
	DriftReason string `json:"driftReason,omitempty"`
	// OnDefaultReason says why a switch is on by default.
	OnDefaultReason string `json:"onDefaultReason,omitempty"`
	// DevOnly marks a development escape hatch. It must default off.
	DevOnly bool `json:"devOnly,omitempty"`
	// Stage is the rollout stage. Empty means dark. See EffectiveStage.
	Stage Stage `json:"stage,omitempty"`
	// KillSwitch marks an operator's emergency switch. Only switches carry it. It must
	// default to the safe value, which for a switch is off, and it is never
	// orgOverridable.
	KillSwitch bool `json:"killSwitch,omitempty"`
	// AgentAccess says whether a model-facing tool may read the value. Empty means
	// none. See EffectiveAgentAccess.
	AgentAccess AgentAccess `json:"agentAccess,omitempty"`
	// OrgOverridable lets an org set its own value. The value may narrow the flag or
	// enable it, but only within the build or environment ceiling. Only switches and
	// choices owned by frontend-plane or model-plane may carry it.
	OrgOverridable bool `json:"orgOverridable,omitempty"`
	// OnLiteral is set on a switch whose reader turns it on for one exact value and no
	// other: "true", or "1" for a gate that reads 1. The reader compares the raw value, so
	// a padded, cased or other spelling reads as off, as it does in the service. Empty
	// means the canonical vocabulary (ParseSwitch) or OnLiteralAnyCase. validateFlag checks
	// the literal against the purpose and against every declared default.
	OnLiteral string `json:"onLiteral,omitempty"`
	// OnLiteralAnyCase is set on a switch whose reader trims surrounding space and folds
	// ASCII case, then turns it on for the literal true only: "TRUE" and " true " are on,
	// and 1, on and yes are off. Its value is always "true". A switch sets OnLiteral or
	// OnLiteralAnyCase, never both.
	OnLiteralAnyCase string `json:"onLiteralAnyCase,omitempty"`
	// ChoiceRead says how a choice's reader compares the raw variable with Values. Empty
	// means trimmed: the reader removes surrounding space and compares the rest exactly.
	// Only choices carry it. See the ChoiceRead constants.
	ChoiceRead ChoiceRead `json:"choiceRead,omitempty"`
}

// File is the document shape of flag-registry.json.
type File struct {
	Schema string `json:"schema"`
	Flags  []Flag `json:"flags"`
	// Unregistered names compose variables that follow the naming convention
	// but have no entry yet. It is a visible baseline and may only shrink.
	Unregistered []string `json:"unregistered"`
	// UnregisteredSwitches names Model Plane compose variables outside the naming
	// convention whose compose default is a boolean-looking value, and that have no
	// entry yet. It is a visible baseline and may only shrink. A convention name
	// belongs in Unregistered instead.
	UnregisteredSwitches []string `json:"unregisteredSwitches"`
}

// Lookup reports a variable's value and whether it is set. os.LookupEnv has this shape.
type Lookup func(name string) (value string, ok bool)

var (
	// ErrUnknownFlag reports a name the registry does not declare.
	ErrUnknownFlag = errors.New("unknown flag")
	// ErrWrongKind reports a typed read against a flag of another kind.
	ErrWrongKind = errors.New("flag has another kind")
	// ErrUnknownChoice reports a choice value outside the flag's Values.
	ErrUnknownChoice = errors.New("value is not an allowed choice")
)

var (
	namePattern    = regexp.MustCompile(`^[A-Z][A-Z0-9_]*$`)
	servicePattern = regexp.MustCompile(`^[a-z][a-z0-9-]*$`)
)

// Registry is a validated, read-only set of flags.
type Registry struct {
	flags                []Flag
	byName               map[string]int
	unregistered         []string
	unregisteredSwitches []string
}

// Load decodes and validates a registry document. Unknown fields and data after
// the document are errors.
func Load(r io.Reader) (*Registry, error) {
	dec := json.NewDecoder(r)
	dec.DisallowUnknownFields()
	var doc File
	if err := dec.Decode(&doc); err != nil {
		return nil, fmt.Errorf("decode flag registry: %w", err)
	}
	if _, err := dec.Token(); !errors.Is(err, io.EOF) {
		return nil, errors.New("decode flag registry: data after the document")
	}
	return New(doc)
}

// New validates a registry document and returns the registry. Every problem is
// reported, not just the first.
func New(doc File) (*Registry, error) {
	var errs []error
	if doc.Schema != SchemaV1 {
		errs = append(errs, fmt.Errorf("schema %q, want %q", doc.Schema, SchemaV1))
	}
	byName := make(map[string]int, len(doc.Flags))
	for i, f := range doc.Flags {
		if i > 0 && f.Name <= doc.Flags[i-1].Name {
			errs = append(errs, fmt.Errorf("flags must be sorted by name with no duplicates: %s follows %s", f.Name, doc.Flags[i-1].Name))
		}
		if _, ok := byName[f.Name]; !ok {
			byName[f.Name] = i
		}
		errs = append(errs, validateFlag(f)...)
	}
	for i, name := range doc.Unregistered {
		if i > 0 && name <= doc.Unregistered[i-1] {
			errs = append(errs, fmt.Errorf("unregistered names must be sorted with no duplicates: %s follows %s", name, doc.Unregistered[i-1]))
		}
		if !IsConventionName(name) {
			errs = append(errs, fmt.Errorf("unregistered %s does not follow the naming convention", name))
		}
		if _, ok := byName[name]; ok {
			errs = append(errs, fmt.Errorf("%s is both registered and unregistered", name))
		}
	}
	for i, name := range doc.UnregisteredSwitches {
		if i > 0 && name <= doc.UnregisteredSwitches[i-1] {
			errs = append(errs, fmt.Errorf("unregisteredSwitches must be sorted with no duplicates: %s follows %s", name, doc.UnregisteredSwitches[i-1]))
		}
		if !namePattern.MatchString(name) {
			errs = append(errs, fmt.Errorf("unregisteredSwitches %q must match %s", name, namePattern))
		}
		if IsConventionName(name) {
			errs = append(errs, fmt.Errorf("unregisteredSwitches %s follows the naming convention; list it in unregistered", name))
		}
		if _, ok := byName[name]; ok {
			errs = append(errs, fmt.Errorf("%s is both registered and in unregisteredSwitches", name))
		}
	}
	if len(errs) > 0 {
		return nil, errors.Join(errs...)
	}
	return &Registry{
		flags:                slices.Clone(doc.Flags),
		byName:               byName,
		unregistered:         slices.Clone(doc.Unregistered),
		unregisteredSwitches: slices.Clone(doc.UnregisteredSwitches),
	}, nil
}

func validateFlag(f Flag) []error {
	var errs []error
	fail := func(format string, args ...any) {
		errs = append(errs, fmt.Errorf("flag %s: %s", f.Name, fmt.Sprintf(format, args...)))
	}
	if !namePattern.MatchString(f.Name) {
		fail("name must match %s", namePattern)
	}
	if !slices.Contains(knownPlanes, f.Owner) {
		fail("unknown owner %q", f.Owner)
	}
	if f.Purpose == "" || f.Purpose != strings.TrimSpace(f.Purpose) || utf8.RuneCountInString(f.Purpose) > maxPurposeRunes {
		fail("purpose must be a trimmed, non-empty line of at most %d characters", maxPurposeRunes)
	}
	if f.CodePath != "" && !validRepoPath(f.CodePath) {
		fail("codePath %q must be a relative path with forward slashes", f.CodePath)
	}
	switch f.Source {
	case SourceCompose:
		if len(f.Declarations) == 0 {
			fail("compose flags need at least one declaration")
		}
	case SourceCode:
		if f.CodePath == "" {
			fail("code flags need a codePath")
		}
		if f.CodeDefault != "" {
			fail("codeDefault applies only to compose flags")
		}
	default:
		fail("unknown source %q", f.Source)
	}
	type slot struct{ service, env string }
	seen := make(map[slot]bool, len(f.Declarations))
	for _, d := range f.Declarations {
		if !servicePattern.MatchString(d.Service) {
			fail("declaration service %q is not a compose service name", d.Service)
		}
		if !namePattern.MatchString(d.Env) {
			fail("declaration env %q must match %s", d.Env, namePattern)
		}
		if !sameDefault(f.Kind, d.Default, f.Default) {
			fail("declaration %s/%s defaults to %q, not %q", d.Service, d.Env, d.Default, f.Default)
		}
		key := slot{d.Service, d.Env}
		if seen[key] {
			fail("duplicate declaration for %s/%s", d.Service, d.Env)
		}
		seen[key] = true
	}
	switch f.Stage {
	case "", StageDark, StageBeta, StageGA:
	default:
		fail("stage %q must be dark, beta or ga", f.Stage)
	}
	switch f.AgentAccess {
	case "", AgentAccessNone, AgentAccessRead:
	default:
		fail("agentAccess %q must be none or read", f.AgentAccess)
	}
	if f.KillSwitch && f.Kind != KindSwitch {
		fail("killSwitch applies only to switch flags")
	}
	if f.KillSwitch && f.OrgOverridable {
		fail("a kill switch may not be orgOverridable")
	}
	if f.OrgOverridable {
		if f.Kind != KindSwitch && f.Kind != KindChoice {
			fail("orgOverridable applies only to switch and choice flags")
		}
		if f.Owner != PlaneFrontend && f.Owner != PlaneModel {
			fail("orgOverridable applies only to frontend-plane and model-plane flags")
		}
	}
	if f.ChoiceRead != "" && f.Kind != KindChoice {
		fail("choiceRead applies only to choice flags")
	}
	switch f.Kind {
	case KindSwitch:
		if len(f.Values) > 0 {
			fail("switch flags take no values")
		}
		on, err := ParseSwitch(f.Default)
		if err != nil {
			fail("default: %v", err)
			return errs
		}
		if on && f.OnDefaultReason == "" {
			fail("the default is on; say why in onDefaultReason")
		}
		if !on && f.OnDefaultReason != "" {
			fail("onDefaultReason is set but the default is off")
		}
		if f.CodeDefault != "" {
			codeOn, err := ParseSwitch(f.CodeDefault)
			switch {
			case err != nil:
				fail("codeDefault: %v", err)
			case codeOn != on && f.DriftReason == "":
				fail("codeDefault differs from the default; say why in driftReason")
			case codeOn == on && f.DriftReason != "":
				fail("driftReason is set but codeDefault matches the default")
			}
		} else if f.DriftReason != "" {
			fail("driftReason needs a codeDefault")
		}
		if f.DevOnly && on {
			fail("devOnly flags must default off")
		}
		if f.KillSwitch && on {
			fail("a kill switch must default to the safe value, off")
		}
		if f.OnLiteral != "" && f.OnLiteralAnyCase != "" {
			fail("set onLiteral or onLiteralAnyCase, not both")
		}
		if f.OnLiteral != "" {
			if f.OnLiteral != "true" && f.OnLiteral != "1" {
				fail("onLiteral %q must be true or 1", f.OnLiteral)
			} else if !namesLiteral(f.Purpose, f.OnLiteral) {
				fail("onLiteral %s: the purpose must say that only the literal %s turns it on", f.OnLiteral, f.OnLiteral)
			}
		}
		if f.OnLiteralAnyCase != "" {
			if f.OnLiteralAnyCase != "true" {
				fail("onLiteralAnyCase %q must be true", f.OnLiteralAnyCase)
			} else if !namesLiteral(f.Purpose, "true") || !strings.Contains(stripTicks(f.Purpose), "in any case") {
				fail("onLiteralAnyCase: the purpose must say that only the literal true, in any case, turns it on")
			}
		}
		if literal := f.literal(); literal != "" {
			field := "onLiteral"
			if f.OnLiteralAnyCase != "" {
				field = "onLiteralAnyCase"
			}
			values := []string{f.Default, f.CodeDefault}
			for _, d := range f.Declarations {
				values = append(values, d.Default)
			}
			for _, v := range values {
				if v != literal && readsOn(v) {
					fail("%s %s: the value %q reads as on, but only the exact value %s turns the switch on", field, literal, v, literal)
				}
			}
		}
	case KindChoice:
		if len(f.Values) == 0 {
			fail("choice flags need values")
		}
		if dup, ok := firstDuplicate(f.Values); ok {
			fail("duplicate value %q", dup)
		}
		if !slices.Contains(f.Values, f.Default) {
			fail("default %q is not one of the values", f.Default)
		}
		switch f.ChoiceRead {
		case "", ChoiceReadExact:
		case ChoiceReadAnyCase:
			for _, v := range f.Values {
				if v != asciiFold(v) {
					fail("choiceRead anyCase needs lowercase values, and %q is not", v)
				}
			}
		default:
			fail("choiceRead %q must be exact or anyCase", f.ChoiceRead)
		}
		if f.ChoiceRead != ChoiceReadExact {
			for _, v := range f.Values {
				if v != strings.TrimSpace(v) {
					fail("value %q has surrounding space, which a trimmed or anyCase choice never reads", v)
				}
			}
		}
		if f.OnDefaultReason != "" || f.DriftReason != "" || f.CodeDefault != "" || f.DevOnly || f.OnLiteral != "" || f.OnLiteralAnyCase != "" {
			fail("only switch flags take onDefaultReason, driftReason, codeDefault, devOnly, onLiteral or onLiteralAnyCase")
		}
	case KindText:
		if len(f.Values) > 0 {
			fail("text flags take no values")
		}
		if f.OnDefaultReason != "" || f.DriftReason != "" || f.CodeDefault != "" || f.DevOnly || f.OnLiteral != "" || f.OnLiteralAnyCase != "" {
			fail("only switch flags take onDefaultReason, driftReason, codeDefault, devOnly, onLiteral or onLiteralAnyCase")
		}
	default:
		fail("unknown kind %q", f.Kind)
	}
	return errs
}

// sameDefault compares two defaults as the flag's kind reads them. Switch values
// compare by meaning, so a blank declaration and 0 both match an off default.
// Other kinds compare as written.
func sameDefault(kind Kind, a, b string) bool {
	if kind != KindSwitch {
		return a == b
	}
	av, errA := ParseSwitch(a)
	bv, errB := ParseSwitch(b)
	return errA == nil && errB == nil && av == bv
}

func firstDuplicate(values []string) (string, bool) {
	for i, v := range values {
		if slices.Contains(values[:i], v) {
			return v, true
		}
	}
	return "", false
}

// validRepoPath accepts a relative, forward-slash path with no empty, "." or
// ".." element, so a codePath cannot point outside the repository.
func validRepoPath(p string) bool {
	if strings.ContainsAny(p, "\\:") || strings.HasPrefix(p, "/") {
		return false
	}
	for _, part := range strings.Split(p, "/") {
		if part == "" || part == "." || part == ".." {
			return false
		}
	}
	return true
}

// EffectiveStage is the flag's rollout stage. An entry that does not say is dark.
func (f Flag) EffectiveStage() Stage {
	if f.Stage == "" {
		return StageDark
	}
	return f.Stage
}

// EffectiveAgentAccess is what a model-facing tool may do with the value. An entry
// that does not say gets none.
func (f Flag) EffectiveAgentAccess() AgentAccess {
	if f.AgentAccess == "" {
		return AgentAccessNone
	}
	return f.AgentAccess
}

// Flags returns every entry, sorted by name.
func (r *Registry) Flags() []Flag {
	out := make([]Flag, len(r.flags))
	for i, f := range r.flags {
		out[i] = cloneFlag(f)
	}
	return out
}

// Flag returns the entry for name.
func (r *Registry) Flag(name string) (Flag, bool) {
	i, ok := r.byName[name]
	if !ok {
		return Flag{}, false
	}
	return cloneFlag(r.flags[i]), true
}

// Unregistered returns the convention names that compose sets but the registry
// does not yet describe.
func (r *Registry) Unregistered() []string {
	return slices.Clone(r.unregistered)
}

// UnregisteredSwitches returns the Model Plane compose switches outside the naming
// convention that compose sets and the registry does not yet describe.
func (r *Registry) UnregisteredSwitches() []string {
	return slices.Clone(r.unregisteredSwitches)
}

// Switch returns the typed value of a switch flag, read as its service reads it. An exact switch
// (OnLiteral set) is on only when the raw value is its literal; any other value, blank-looking
// whitespace included, reads as off. An any-case switch (OnLiteralAnyCase set) is on when the value,
// trimmed and folded, is the literal. A canonical switch reads the ParseSwitch vocabulary. An unset
// or empty variable takes the registry default for every kind, as compose's ${NAME:-default} does.
func (r *Registry) Switch(name string, lookup Lookup) (bool, error) {
	f, err := r.kindOf(name, KindSwitch)
	if err != nil {
		return false, err
	}
	raw := valueOr(f, lookup)
	if literal := f.literal(); literal != "" {
		return raw == literal, nil
	}
	on, err := ParseSwitch(raw)
	if err != nil {
		return false, fmt.Errorf("%s: %w", name, err)
	}
	return on, nil
}

// Choice returns the typed value of a choice flag, read as its service reads it, and rejects a
// value outside Values. An exact choice compares the raw value, so a padded value is refused. An
// any-case choice trims and folds it first. A trimmed choice trims it. See ChoiceRead.
func (r *Registry) Choice(name string, lookup Lookup) (string, error) {
	f, err := r.kindOf(name, KindChoice)
	if err != nil {
		return "", err
	}
	v := valueOr(f, lookup)
	if !slices.Contains(f.Values, v) {
		return "", fmt.Errorf("%s: %w: %q", name, ErrUnknownChoice, v)
	}
	return v, nil
}

// Text returns the value of a text flag, with surrounding space trimmed.
func (r *Registry) Text(name string, lookup Lookup) (string, error) {
	f, err := r.kindOf(name, KindText)
	if err != nil {
		return "", err
	}
	return valueOr(f, lookup), nil
}

func (r *Registry) kindOf(name string, want Kind) (Flag, error) {
	f, ok := r.Flag(name)
	if !ok {
		return Flag{}, fmt.Errorf("%w: %s", ErrUnknownFlag, name)
	}
	if f.Kind != want {
		return Flag{}, fmt.Errorf("%w: %s is a %s flag", ErrWrongKind, name, f.Kind)
	}
	return f, nil
}

// valueOr returns the value a deployment sees for f, as the flag's service reads it. An unset or
// empty variable takes the default, as compose's ${NAME:-default} does. Otherwise the read depends
// on the flag's readMode: an exact switch and an exact choice keep the raw value; an any-case switch
// and an any-case choice trim and fold ASCII case, and keep a value that is blank afterwards, so the
// reader sees it and does not fall back to the default; every other flag trims the value and takes
// the default when the trimmed value is blank.
func valueOr(f Flag, lookup Lookup) string {
	if lookup != nil {
		if raw, ok := lookup(f.Name); ok && raw != "" {
			switch f.read() {
			case readRaw:
				return raw
			case readFolded:
				return asciiFold(strings.TrimSpace(raw))
			default:
				if trimmed := strings.TrimSpace(raw); trimmed != "" {
					return trimmed
				}
			}
		}
	}
	return f.Default
}

// readMode is how a flag's service compares the raw variable.
type readMode int

const (
	// readTrimmed trims surrounding space, then compares. Every flag without an exact or any-case
	// reader reads this way.
	readTrimmed readMode = iota
	// readRaw compares the value as written.
	readRaw
	// readFolded trims surrounding space and folds ASCII case, then compares.
	readFolded
)

// read reports how the flag's service compares the raw variable.
func (f Flag) read() readMode {
	switch {
	case f.OnLiteral != "" || f.ChoiceRead == ChoiceReadExact:
		return readRaw
	case f.OnLiteralAnyCase != "" || f.ChoiceRead == ChoiceReadAnyCase:
		return readFolded
	default:
		return readTrimmed
	}
}

// literal is the value that turns an exact or any-case switch on, or "" for a canonical switch.
func (f Flag) literal() string {
	if f.OnLiteral != "" {
		return f.OnLiteral
	}
	return f.OnLiteralAnyCase
}

// asciiFold lowercases the ASCII letters of s and leaves every other rune as it is, as Rust's
// to_ascii_lowercase and eq_ignore_ascii_case do.
func asciiFold(s string) string {
	return strings.Map(func(r rune) rune {
		if 'A' <= r && r <= 'Z' {
			return r + ('a' - 'A')
		}
		return r
	}, s)
}

// stripTicks removes the backticks a purpose puts around a literal, so "`true`" reads as true.
func stripTicks(s string) string {
	return strings.ReplaceAll(s, "`", "")
}

// namesLiteral reports whether a purpose names the literal that turns the switch on. The word
// "literal" matches in any case, so "Literal true only" names it as "literal true" does, and the
// literal itself must match exactly and end at a word boundary: "literal trueish" does not name true.
func namesLiteral(purpose, literal string) bool {
	pattern := regexp.MustCompile(`(?i:literal) ` + regexp.QuoteMeta(literal) + `\b`)
	return pattern.MatchString(stripTicks(purpose))
}

func cloneFlag(f Flag) Flag {
	f.Values = slices.Clone(f.Values)
	f.Declarations = slices.Clone(f.Declarations)
	return f
}
