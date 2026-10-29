package demo

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/match"
	"github.com/triodelab/anbud/internal/normalize"
	"github.com/triodelab/anbud/internal/rawstore"
)

// Limits of a request.
const (
	DefaultLimit = 50
	MaxLimit     = 200
	DefaultScan  = 300
	// MaxScan is how many stored notices one request may read.
	MaxScan = rawstore.MaxRecent

	maxEntries      = 50
	maxKeywordRunes = 80
	maxLeadDays     = 3650
	maxAmount       = 1_000_000_000_000 // in whole currency units, so that øre cannot overflow
	maxFieldErrors  = 20
)

// FieldError names a field of a request and why it is refused. The reason is fixed text: the value
// is never echoed, because a key pasted into the wrong field would be printed back by the error
// that refuses it.
type FieldError struct {
	Field  string `json:"field"`
	Reason string `json:"reason"`
}

// Request is a validated, normalised request to match a profile against the stored notices.
type Request struct {
	Paginate       bool
	Cursor         string
	Sort           string
	Filters        MatchFilters
	User           string // authenticated actor, never decoded from JSON
	TrackedOnly    bool
	Profile        match.Profile
	Strictness     match.Strictness
	StrictnessName string
	// ShowAll lists every notice, with the ones the gate fails and the ones that do not fit as
	// well. Without it the list holds only notices a company could still bid on that fit.
	ShowAll bool
	Limit   int // results returned
	Scan    int // stored notices read
	// Org is the organisation the request is made for, and so whose uploaded tender documents are read.
	// It comes from the authenticated request (X-Org-ID), never from the body: DecodeMatchRequest leaves
	// it empty, and a request without one reads no documents.
	Org string
}

var strictnessNames = []string{"strict", "balanced", "wide"}

var strictnessByName = map[string]match.Strictness{
	"strict": match.Strict, "balanced": match.Balanced, "wide": match.Wide,
}

// strictnessFrom reads the slider's step and its name: balanced when the request gives none. A name
// that is not one of the steps is a field error, and the step falls back to balanced.
func strictnessFrom(v *validator, name *string) (match.Strictness, string) {
	if name != nil {
		if s, ok := strictnessByName[*name]; ok {
			return s, strictnessName(s)
		}
		v.add("strictness", "must be one of "+strings.Join(strictnessNames, ", "))
	}
	return match.Balanced, strictnessName(match.Balanced)
}

// wire shapes. Pointers tell "not given" from zero.
type matchingContextRequest struct {
	CompanyKeywords []string `json:"company_keywords"`
	UserKeywords    []string `json:"user_keywords"`
}
type MatchFilters struct {
	Query    string `json:"query"`
	Buyer    string `json:"buyer"`
	Region   string `json:"region"`
	CPV      string `json:"cpv"`
	Status   string `json:"status"`
	Deadline string `json:"deadline"`
	Verdict  string `json:"verdict"`
}
type matchRequest struct {
	Paginate    bool                    `json:"paginate"`
	Cursor      string                  `json:"cursor"`
	Sort        string                  `json:"sort"`
	Filters     MatchFilters            `json:"filters"`
	TrackedOnly bool                    `json:"tracked_only"`
	Context     *matchingContextRequest `json:"context"`
	Profile     *profileRequest         `json:"profile"`
	Strictness  *string                 `json:"strictness"`
	Show        *string                 `json:"show"`
	Limit       *int                    `json:"limit"`
	Scan        *int                    `json:"scan"`
}

type profileRequest struct {
	OrgNumber   string                     `json:"org_number"`
	CPV         []string                   `json:"cpv"`
	ExcludedCPV []string                   `json:"excluded_cpv"`
	NUTS        []string                   `json:"nuts"`
	ValueRange  *valueRangeRequest         `json:"value_range"`
	Keywords    []string                   `json:"keywords"`
	Held        []string                   `json:"held"`
	LeadTimes   map[string]leadTimeRequest `json:"lead_times"`
}

type valueRangeRequest struct {
	Currency string `json:"currency"`
	Min      *int64 `json:"min"`
	Max      *int64 `json:"max"`
}

type leadTimeRequest struct {
	Days               *int `json:"days"`
	EquivalentAccepted bool `json:"equivalent_accepted"`
}

// validator collects field errors, the first maxFieldErrors of them.
type validator struct{ errs []FieldError }

func (v *validator) add(field, reason string) {
	if len(v.errs) < maxFieldErrors {
		v.errs = append(v.errs, FieldError{Field: field, Reason: reason})
	}
}

// DecodeMatchRequest reads and validates a request body. Unknown fields are refused, every problem
// is reported at once (up to maxFieldErrors), and none of them quotes a value.
func DecodeMatchRequest(body []byte) (Request, []FieldError) {
	var wire matchRequest
	if e := decodeStrict(body, &wire); e != nil {
		return Request{}, []FieldError{*e}
	}

	var v validator
	r := Request{Limit: DefaultLimit, Scan: DefaultScan, TrackedOnly: wire.TrackedOnly, Paginate: wire.Paginate, Cursor: wire.Cursor, Sort: wire.Sort, Filters: wire.Filters}
	if len(r.Cursor) > 512 {
		v.add("cursor", "is too long")
	}
	if r.Cursor != "" && !r.Paginate {
		v.add("paginate", "is required with a cursor")
	}
	if !validMatchFilters(r) {
		v.add("filters", "contains an unsupported filter or sort value")
	}
	r.Strictness, r.StrictnessName = strictnessFrom(&v, wire.Strictness)
	if wire.Show != nil {
		switch *wire.Show {
		case "relevant":
		case "all":
			r.ShowAll = true
		default:
			v.add("show", "must be one of relevant, all")
		}
	}
	if wire.Limit != nil {
		if *wire.Limit < 1 || *wire.Limit > MaxLimit {
			v.add("limit", fmt.Sprintf("must be between 1 and %d", MaxLimit))
		} else {
			r.Limit = *wire.Limit
		}
	}
	if wire.Scan != nil {
		if *wire.Scan < 1 || *wire.Scan > MaxScan {
			v.add("scan", fmt.Sprintf("must be between 1 and %d", MaxScan))
		} else {
			r.Scan = *wire.Scan
		}
	}
	if !wire.TrackedOnly {
		if wire.Profile == nil {
			v.add("profile", "is required")
		} else {
			r.Profile = buildProfile(&v, wire.Profile)
		}
	}
	if wire.Context != nil {
		contextValidator := validator{}
		r.Profile.SEOKeywords = keywords(&contextValidator, wire.Context.CompanyKeywords)
		r.Profile.UserKeywords = keywords(&contextValidator, wire.Context.UserKeywords)
		for _, err := range contextValidator.errs {
			v.add(strings.Replace(err.Field, "profile.keywords", "context.keywords", 1), err.Reason)
		}
	}
	if len(v.errs) > 0 {
		return Request{}, v.errs
	}
	return r, nil
}

// decodeStrict reads exactly one JSON value into into. Unknown fields and a second value are
// refused. The error is fixed text and never quotes the body.
func decodeStrict(body []byte, into any) *FieldError {
	dec := json.NewDecoder(bytes.NewReader(body))
	dec.DisallowUnknownFields()
	if err := dec.Decode(into); err != nil {
		e := decodeError(err)
		return &e
	}
	if err := dec.Decode(&struct{}{}); !errors.Is(err, io.EOF) {
		return &FieldError{Reason: "the body has more than one JSON value"}
	}
	return nil
}

// companyRequest is the body of a company lookup.
type companyRequest struct {
	OrgNumber *string `json:"org_number"`
}

// DecodeCompanyRequest reads the body of a company lookup and returns the organisation number,
// normalised and with its check digit verified. The number is in the body, not in the URL, so that
// it stays out of access logs: for a sole trader it identifies a person's business.
func DecodeCompanyRequest(body []byte) (string, []FieldError) {
	var wire companyRequest
	if e := decodeStrict(body, &wire); e != nil {
		return "", []FieldError{*e}
	}
	if wire.OrgNumber == nil || strings.TrimSpace(*wire.OrgNumber) == "" {
		return "", []FieldError{{Field: "org_number", Reason: "is required"}}
	}
	number, err := normalize.OrgNumber(*wire.OrgNumber)
	switch {
	case errors.Is(err, normalize.ErrOrgNumberChecksum):
		return "", []FieldError{{Field: "org_number", Reason: "has a wrong check digit"}}
	case err != nil:
		return "", []FieldError{{Field: "org_number", Reason: "is not a Norwegian organisation number"}}
	}
	return number, nil
}

// decodeError maps a JSON decoding error to a field error with fixed text. A type error names the
// field from the request type's own tags, not from the body.
func decodeError(err error) FieldError {
	var typeErr *json.UnmarshalTypeError
	switch {
	case errors.As(err, &typeErr) && typeErr.Field == "":
		return FieldError{Reason: "the body must be a JSON object"}
	case errors.As(err, &typeErr):
		return FieldError{Field: typeErr.Field, Reason: "has the wrong type"}
	case strings.HasPrefix(err.Error(), "json: unknown field"):
		return FieldError{Reason: "the body has a field this endpoint does not know"}
	}
	return FieldError{Reason: "the body is not valid JSON"}
}

func buildProfile(v *validator, in *profileRequest) match.Profile {
	var p match.Profile

	if org := strings.TrimSpace(in.OrgNumber); org != "" {
		number, err := normalize.OrgNumber(org)
		switch {
		case errors.Is(err, normalize.ErrOrgNumberChecksum):
			v.add("profile.org_number", "has a wrong check digit")
		case err != nil:
			v.add("profile.org_number", "is not a Norwegian organisation number")
		default:
			p.OrgNumber = number
		}
	}

	p.CPV = codes(v, "profile.cpv", in.CPV, cpvCode)
	p.ExcludedCPV = codes(v, "profile.excluded_cpv", in.ExcludedCPV, cpvCode)
	p.NUTS = codes(v, "profile.nuts", in.NUTS, nutsCode)
	p.Keywords = keywords(v, in.Keywords)
	p.MinValue, p.MaxValue = valueRange(v, in.ValueRange)
	p.Held, p.LeadTimes = capabilities(v, in)

	if len(in.CPV) == 0 && len(in.Keywords) == 0 {
		v.add("profile", "needs at least one CPV code or one keyword to match on")
	}
	return p
}

// A normaliser turns one entry into its canonical form, or says why it cannot.
type normaliser func(raw string) (string, string)

func cpvCode(raw string) (string, string) {
	code, err := normalize.CPV(raw)
	if err != nil {
		return "", "is not a CPV code"
	}
	return code, ""
}

func nutsCode(raw string) (string, string) {
	code, err := normalize.NUTS(raw)
	switch {
	case errors.Is(err, normalize.ErrNUTSExtraRegio):
		return "", "is extra-regio and names no place"
	case err != nil:
		return "", "is not a NUTS code"
	}
	return code, ""
}

// codes normalises a list of codes, each once, in the order given. An entry that is refused is
// reported by its index.
func codes(v *validator, field string, raw []string, normalise normaliser) []string {
	if len(raw) > maxEntries {
		v.add(field, fmt.Sprintf("has more than %d entries", maxEntries))
		return nil
	}
	var out []string
	for i, r := range raw {
		code, reason := normalise(r)
		if reason != "" {
			v.add(fmt.Sprintf("%s[%d]", field, i), reason)
			continue
		}
		if !slices.Contains(out, code) {
			out = append(out, code)
		}
	}
	return out
}

func keywords(v *validator, raw []string) []string {
	if len(raw) > maxEntries {
		v.add("profile.keywords", fmt.Sprintf("has more than %d entries", maxEntries))
		return nil
	}
	var out []string
	seen := map[string]bool{}
	for i, r := range raw {
		k := strings.TrimSpace(r)
		switch {
		case k == "":
			v.add(fmt.Sprintf("profile.keywords[%d]", i), "is empty")
			continue
		case utf8.RuneCountInString(k) > maxKeywordRunes:
			v.add(fmt.Sprintf("profile.keywords[%d]", i), fmt.Sprintf("is longer than %d characters", maxKeywordRunes))
			continue
		}
		if key := strings.ToLower(k); !seen[key] {
			seen[key] = true
			out = append(out, k)
		}
	}
	return out
}

func valueRange(v *validator, in *valueRangeRequest) (lo, hi *eforms.Amount) {
	if in == nil {
		return nil, nil
	}
	currency := strings.ToUpper(strings.TrimSpace(in.Currency))
	if len(currency) != 3 || strings.Trim(currency, "ABCDEFGHIJKLMNOPQRSTUVWXYZ") != "" {
		v.add("profile.value_range.currency", "is not a 3-letter currency code")
		return nil, nil
	}
	if in.Min == nil && in.Max == nil {
		v.add("profile.value_range", "needs a min or a max")
		return nil, nil
	}
	bound := func(field string, n *int64) *eforms.Amount {
		if n == nil {
			return nil
		}
		if *n < 0 || *n > maxAmount {
			v.add("profile.value_range."+field, fmt.Sprintf("must be between 0 and %d", int64(maxAmount)))
			return nil
		}
		return &eforms.Amount{Minor: *n * 100, Currency: currency}
	}
	lo, hi = bound("min", in.Min), bound("max", in.Max)
	if lo != nil && hi != nil && lo.Minor > hi.Minor {
		v.add("profile.value_range", "has a min above its max")
		return nil, nil
	}
	return lo, hi
}

func knownCapability(name string) (match.Capability, bool) {
	c := match.Capability(name)
	_, ok := match.DefaultLeadTimes[c]
	return c, ok
}

func capabilities(v *validator, in *profileRequest) (map[match.Capability]bool, map[match.Capability]match.LeadTime) {
	var held map[match.Capability]bool
	if len(in.Held) > len(match.DefaultLeadTimes) {
		v.add("profile.held", fmt.Sprintf("has more than %d entries", len(match.DefaultLeadTimes)))
	} else {
		for i, name := range in.Held {
			c, ok := knownCapability(name)
			if !ok {
				v.add(fmt.Sprintf("profile.held[%d]", i), "is not a known capability")
				continue
			}
			if held == nil {
				held = map[match.Capability]bool{}
			}
			held[c] = true
		}
	}

	var leads map[match.Capability]match.LeadTime
	names := make([]string, 0, len(in.LeadTimes))
	for name := range in.LeadTimes {
		names = append(names, name)
	}
	slices.Sort(names)
	unknown := false
	for _, name := range names {
		c, ok := knownCapability(name)
		if !ok {
			unknown = true
			continue
		}
		lt := in.LeadTimes[name]
		field := "profile.lead_times." + name + ".days"
		switch {
		case lt.Days == nil:
			v.add(field, "is required")
			continue
		case *lt.Days < 0 || *lt.Days > maxLeadDays:
			v.add(field, fmt.Sprintf("must be between 0 and %d", maxLeadDays))
			continue
		}
		if leads == nil {
			leads = map[match.Capability]match.LeadTime{}
		}
		leads[c] = match.LeadTime{Days: *lt.Days, EquivalentAccepted: lt.EquivalentAccepted}
	}
	if unknown {
		v.add("profile.lead_times", "has a key that is not a known capability")
	}
	return held, leads
}
