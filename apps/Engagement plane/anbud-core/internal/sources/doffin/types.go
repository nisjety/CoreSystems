package doffin

import (
	"bytes"
	"encoding/json"
	"fmt"
	"math"
	"regexp"
	"strconv"
	"strings"
	"time"
	"unicode"
)

// OneOrMany decodes a JSON value that may be an array, a single value or null.
// The published spec types some lists as single objects (and `hits` itself that
// way), so every list the API returns decodes through this rather than failing
// a whole page over the mismatch.
type OneOrMany[T any] []T

// UnmarshalJSON accepts an array, a single value, or null.
func (o *OneOrMany[T]) UnmarshalJSON(data []byte) error {
	data = bytes.TrimSpace(data)
	switch {
	case len(data) == 0 || bytes.Equal(data, []byte("null")):
		*o = nil
	case data[0] == '[':
		var many []T
		if err := json.Unmarshal(data, &many); err != nil {
			return err
		}
		*o = many
	default:
		var one T
		if err := json.Unmarshal(data, &one); err != nil {
			return err
		}
		*o = []T{one}
	}
	return nil
}

// Number is a JSON number that may arrive quoted, empty or null. It keeps the
// text, so decimals and large values survive exactly.
type Number string

// String returns the number as the API sent it.
func (n Number) String() string { return string(n) }

// UnmarshalJSON accepts a number, a numeric string, an empty string, or null.
func (n *Number) UnmarshalJSON(b []byte) error {
	s := strings.TrimSpace(string(b))
	if s == "null" {
		*n = ""
		return nil
	}
	if strings.HasPrefix(s, `"`) {
		var unquoted string
		if err := json.Unmarshal(b, &unquoted); err != nil {
			return err
		}
		s = strings.TrimSpace(unquoted)
	}
	if s == "" {
		*n = ""
		return nil
	}
	if f, err := strconv.ParseFloat(s, 64); err != nil || math.IsNaN(f) || math.IsInf(f, 0) {
		return fmt.Errorf("doffin: %q is not a number", clip(s))
	}
	*n = Number(s)
	return nil
}

// Count is an integer the published spec promises but that the API might
// deliver as a string, a float or null; decoding must not fail a whole page
// over it. Non-finite and out-of-range values are rejected.
type Count int64

// UnmarshalJSON accepts a number, a numeric string, or null.
func (c *Count) UnmarshalJSON(b []byte) error {
	s := strings.Trim(strings.TrimSpace(string(b)), `"`)
	if s == "" || s == "null" {
		*c = 0
		return nil
	}
	if n, err := strconv.ParseInt(s, 10, 64); err == nil {
		*c = Count(n)
		return nil
	}
	f, err := strconv.ParseFloat(s, 64)
	if err != nil || math.IsNaN(f) || math.IsInf(f, 0) || f < math.MinInt64 || f >= math.MaxInt64 {
		return fmt.Errorf("doffin: %q is not a count", clip(s))
	}
	*c = Count(f)
	return nil
}

// Organization is a buyer or a lot winner as the search API reports it.
type Organization struct {
	ID             string `json:"id"`
	OrganizationID string `json:"organizationId"`
	Name           string `json:"name"`
}

// Money is an estimated value.
type Money struct {
	CurrencyCode string `json:"currencyCode"`
	Amount       Number `json:"amount"`
}

// TenderCount is one entry of allReceivedTenders.
type TenderCount struct {
	Type  string `json:"type"`
	Total Count  `json:"total"`
}

// Lot is a lot summary inside a search hit.
type Lot struct {
	Heading     string                  `json:"heading"`
	Description string                  `json:"description"`
	Winner      OneOrMany[Organization] `json:"winner"`
}

// Hit is one notice in a search result.
//
// Shapes below were checked against 100 live notices on 2026-10-09. Status is
// set only on competition notices and Deadline is not a submission deadline on
// result notices, so neither can be read without the notice type. IssueDate is a
// full timestamp while PublicationDate is a plain date. LimitedDataFlag and
// ClassicURL were null in every sample; they are kept verbatim.
type Hit struct {
	ID                 string                  `json:"id"`
	Buyer              OneOrMany[Organization] `json:"buyer"`
	Heading            string                  `json:"heading"`
	Description        string                  `json:"description"`
	LocationID         OneOrMany[string]       `json:"locationId"`
	EstimatedValue     *Money                  `json:"estimatedValue"`
	Type               string                  `json:"type"`
	AllTypes           OneOrMany[string]       `json:"allTypes"`
	Status             *string                 `json:"status"`
	IssueDate          string                  `json:"issueDate"`
	Deadline           *string                 `json:"deadline"`
	PublicationDate    string                  `json:"publicationDate"`
	ReceivedTenders    Count                   `json:"receivedTenders"`
	AllReceivedTenders OneOrMany[TenderCount]  `json:"allReceivedTenders"`
	CPVCodes           OneOrMany[string]       `json:"cpvCodes"`
	LimitedDataFlag    json.RawMessage         `json:"limitedDataFlag"`
	ClassicURL         json.RawMessage         `json:"doffinClassicUrl"`
	Lots               OneOrMany[Lot]          `json:"lots"`
}

// SearchResponse is a page of search results. Raw keeps the exact bytes the
// API returned so callers can store them untouched, even when decoding failed.
// ContentType and FetchedAt are the response evidence: the Content-Type header
// and the client clock in UTC at receipt. Both are set on success and on the
// DecodeError path.
type SearchResponse struct {
	NumHitsTotal      Count           `json:"numHitsTotal"`
	NumHitsAccessible Count           `json:"numHitsAccessible"`
	Hits              OneOrMany[Hit]  `json:"hits"`
	Raw               json.RawMessage `json:"-"`
	ContentType       string          `json:"-"`
	FetchedAt         time.Time       `json:"-"`
}

// Notice is a downloaded notice, kept exactly as received.
type Notice struct {
	ID          string
	ContentType string
	Body        []byte
	SHA256      string
	FetchedAt   time.Time
}

// SearchParams are the filters of GET /search. Zero values are omitted.
type SearchParams struct {
	SearchString string
	Types        []string
	Statuses     []string
	CPVCodes     []string
	Locations    []string
	SortBy       string

	// Page is a pointer so that an unset page is left out of the query while an explicit
	// 0 is still sent as given. Paging is 1-based and the API refuses page 0 with 400.
	Page           *int
	NumHitsPerPage int

	IssueDateFrom time.Time
	IssueDateTo   time.Time

	EstimatedValueFrom *int64
	EstimatedValueTo   *int64
}

const (
	maxSearchStringRunes = 200
	dateLayout           = "2006-01-02"
	maxClippedRunes      = 40
)

var (
	tokenPattern    = regexp.MustCompile(`^[A-Z][A-Z0-9_]{0,63}$`)
	cpvPattern      = regexp.MustCompile(`^\d{1,8}(-\d)?$`)
	locationPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{0,31}$`)
	noticeIDPattern = regexp.MustCompile(`^\d{4}-\d{4,9}$`)
)

// query validates the parameters and encodes them. Everything that reaches the
// URL has been checked against a strict pattern, so a value can never smuggle
// in another parameter.
func (p SearchParams) query() (map[string][]string, error) {
	q := make(map[string][]string)
	invalid := func(format string, args ...any) error {
		return fmt.Errorf("%w: %s", ErrInvalidParams, fmt.Sprintf(format, args...))
	}

	if p.NumHitsPerPage != 0 {
		if p.NumHitsPerPage < 0 || p.NumHitsPerPage > MaxHitsPerPage {
			return nil, invalid("numHitsPerPage must be between 1 and %d", MaxHitsPerPage)
		}
		q["numHitsPerPage"] = []string{strconv.Itoa(p.NumHitsPerPage)}
	}
	if p.Page != nil {
		if *p.Page < 0 {
			return nil, invalid("page must not be negative")
		}
		q["page"] = []string{strconv.Itoa(*p.Page)}
	}
	if p.SortBy != "" {
		if !tokenPattern.MatchString(p.SortBy) {
			return nil, invalid("sortBy is not a valid token")
		}
		q["sortBy"] = []string{p.SortBy}
	}
	if p.SearchString != "" {
		if len([]rune(p.SearchString)) > maxSearchStringRunes || hasControl(p.SearchString) {
			return nil, invalid("searchString must be at most %d characters without control characters", maxSearchStringRunes)
		}
		q["searchString"] = []string{p.SearchString}
	}

	for _, set := range []struct {
		key     string
		values  []string
		pattern *regexp.Regexp
	}{
		{"type", p.Types, tokenPattern},
		{"status", p.Statuses, tokenPattern},
		{"cpvCode", p.CPVCodes, cpvPattern},
		{"location", p.Locations, locationPattern},
	} {
		for _, v := range set.values {
			if !set.pattern.MatchString(v) {
				return nil, invalid("%s contains a value that is not allowed", set.key)
			}
			q[set.key] = append(q[set.key], v)
		}
	}

	if !p.IssueDateFrom.IsZero() {
		q["issueDateFrom"] = []string{p.IssueDateFrom.Format(dateLayout)}
	}
	if !p.IssueDateTo.IsZero() {
		q["issueDateTo"] = []string{p.IssueDateTo.Format(dateLayout)}
	}
	if !p.IssueDateFrom.IsZero() && !p.IssueDateTo.IsZero() &&
		p.IssueDateFrom.Format(dateLayout) > p.IssueDateTo.Format(dateLayout) {
		return nil, invalid("issueDateFrom is after issueDateTo")
	}

	for key, v := range map[string]*int64{"estimatedValueFrom": p.EstimatedValueFrom, "estimatedValueTo": p.EstimatedValueTo} {
		if v == nil {
			continue
		}
		if *v < 0 {
			return nil, invalid("%s must not be negative", key)
		}
		q[key] = []string{strconv.FormatInt(*v, 10)}
	}
	if p.EstimatedValueFrom != nil && p.EstimatedValueTo != nil && *p.EstimatedValueFrom > *p.EstimatedValueTo {
		return nil, invalid("estimatedValueFrom is greater than estimatedValueTo")
	}
	return q, nil
}

func hasControl(s string) bool {
	for _, r := range s {
		if unicode.IsControl(r) {
			return true
		}
	}
	return false
}

// clip bounds a value quoted in an error message.
func clip(s string) string {
	if r := []rune(s); len(r) > maxClippedRunes {
		return string(r[:maxClippedRunes]) + "…"
	}
	return s
}
