// Package brreg is a filtered client for the open Enhetsregisteret API
// (Brønnøysundregistrene), purpose-built for company lead-building.
//
// Unlike org-core's navn-only, size-20 lookup client, this client filters by
// næringskode / kommunenummer / organisasjonsform / employee-range /
// registration-date and paginates within the API's hard limits.
//
// COMPANY DATA ONLY: it calls only `/enheter` (company records). It never calls
// `/enheter/{orgnr}/roller`, so it never fetches a person, a role, or a
// fødselsnummer (birth number). The [Company] record carries no person/PII field.
//
// Docs: https://data.brreg.no/enhetsregisteret/api/docs/index.html
package brreg

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"
)

// DefaultBaseURL is the public Enhetsregisteret API root (no API key required).
const DefaultBaseURL = "https://data.brreg.no/enhetsregisteret/api"

// MaxResultWindow is Enhetsregisteret's deep-paging cap: (page+1)*size must not
// exceed it, or the API returns HTTP 400. Beyond it, the daily download service
// is the documented path — there is NO searchAfter cursor for /enheter.
const MaxResultWindow = 10_000

const defaultSize = 20

// maxSize is a sane client-side page-size ceiling.
const maxSize = 100

var (
	// ErrEmployeeBandUnsupported reports an unsatisfiable employee filter:
	// Enhetsregisteret v2 rejects fra/tilAntallAnsatte values 1–4 with HTTP 400
	// (antallAnsatte is null for companies with 0–4 employees). The caller should
	// use 0 or >= 5, or drop the bound.
	ErrEmployeeBandUnsupported = errors.New("brreg: antallAnsatte filter values 1-4 are not supported by Enhetsregisteret (use 0 or >=5)")
	// ErrDeepPagingLimit reports that (page+1)*size exceeds MaxResultWindow.
	ErrDeepPagingLimit = errors.New("brreg: (page+1)*size exceeds the 10000-result window; narrow the filter or use the download service")
)

// Company is the COMPANY-ONLY lead record. It deliberately carries no person,
// role (roller), contact-person, or birth-number (fødselsnummer) field — those
// live on the /roller endpoint this client never calls. `Navn` is the company's
// own name, never a person's.
type Company struct {
	Organisasjonsnummer string `json:"organisasjonsnummer"`
	Navn                string `json:"navn"`
	Organisasjonsform   string `json:"organisasjonsform,omitempty"`
	Naeringskode        string `json:"naeringskode,omitempty"`
	NaeringBeskrivelse  string `json:"naering_beskrivelse,omitempty"`
	Kommunenummer       string `json:"kommunenummer,omitempty"`
	Poststed            string `json:"poststed,omitempty"`
	AntallAnsatte       *int   `json:"antall_ansatte,omitempty"`
	Registreringsdato   string `json:"registreringsdato,omitempty"`
	Hjemmeside          string `json:"hjemmeside,omitempty"`
	Konkurs             bool   `json:"konkurs"`
	UnderAvvikling      bool   `json:"under_avvikling"`
}

// SearchFilter holds the supported Enhetsregisteret facets. Empty fields are
// omitted from the request. Dates are ISO `YYYY-MM-DD`.
type SearchFilter struct {
	Naeringskode         string
	Kommunenummer        string
	Organisasjonsform    string
	FraAntallAnsatte     *int
	TilAntallAnsatte     *int
	FraRegistreringsdato string
	TilRegistreringsdato string
	Page                 int
	Size                 int
}

// SearchPage is one page of company results plus the paging metadata.
type SearchPage struct {
	Companies     []Company `json:"companies"`
	Page          int       `json:"page"`
	Size          int       `json:"size"`
	TotalElements int       `json:"total_elements"`
	TotalPages    int       `json:"total_pages"`
}

// Client is a lightweight HTTP client for the filtered /enheter search.
type Client struct {
	baseURL    string
	httpClient *http.Client
}

func NewClient() *Client { return NewClientWithBaseURL(DefaultBaseURL) }

// NewClientWithBaseURL allows tests to point at an httptest server.
func NewClientWithBaseURL(baseURL string) *Client {
	return &Client{
		baseURL:    strings.TrimRight(strings.TrimSpace(baseURL), "/"),
		httpClient: &http.Client{Timeout: 15 * time.Second},
	}
}

func inEmployeeBand(v *int) bool { return v != nil && *v >= 1 && *v <= 4 }

func effectiveSize(size int) int {
	if size <= 0 {
		return defaultSize
	}
	if size > maxSize {
		return maxSize
	}
	return size
}

// Validate enforces the API's hard limits client-side so callers get a typed
// error instead of an opaque HTTP 400.
func (f SearchFilter) Validate() error {
	if inEmployeeBand(f.FraAntallAnsatte) || inEmployeeBand(f.TilAntallAnsatte) {
		return ErrEmployeeBandUnsupported
	}
	if f.Page < 0 {
		return fmt.Errorf("brreg: page must be >= 0")
	}
	if (f.Page+1)*effectiveSize(f.Size) > MaxResultWindow {
		return ErrDeepPagingLimit
	}
	return nil
}

func (f SearchFilter) query() url.Values {
	q := url.Values{}
	set := func(k, v string) {
		if strings.TrimSpace(v) != "" {
			q.Set(k, v)
		}
	}
	set("naeringskode", f.Naeringskode)
	set("kommunenummer", f.Kommunenummer)
	set("organisasjonsform", f.Organisasjonsform)
	set("fraRegistreringsdatoEnhetsregisteret", f.FraRegistreringsdato)
	set("tilRegistreringsdatoEnhetsregisteret", f.TilRegistreringsdato)
	if f.FraAntallAnsatte != nil {
		q.Set("fraAntallAnsatte", strconv.Itoa(*f.FraAntallAnsatte))
	}
	if f.TilAntallAnsatte != nil {
		q.Set("tilAntallAnsatte", strconv.Itoa(*f.TilAntallAnsatte))
	}
	q.Set("size", strconv.Itoa(effectiveSize(f.Size)))
	if f.Page > 0 {
		q.Set("page", strconv.Itoa(f.Page))
	}
	return q
}

// rawSearchResponse mirrors only the parts of the /enheter response we map.
type rawSearchResponse struct {
	Embedded struct {
		Enheter []rawEnhet `json:"enheter"`
	} `json:"_embedded"`
	Page struct {
		Number        int `json:"number"`
		Size          int `json:"size"`
		TotalPages    int `json:"totalPages"`
		TotalElements int `json:"totalElements"`
	} `json:"page"`
}

// rawEnhet mirrors only the COMPANY fields we surface — never roller/persons.
type rawEnhet struct {
	Organisasjonsnummer string `json:"organisasjonsnummer"`
	Navn                string `json:"navn"`
	Organisasjonsform   struct {
		Kode string `json:"kode"`
	} `json:"organisasjonsform"`
	Naeringskode1 *struct {
		Kode        string `json:"kode"`
		Beskrivelse string `json:"beskrivelse"`
	} `json:"naeringskode1"`
	Forretningsadresse *struct {
		Kommunenummer string `json:"kommunenummer"`
		Poststed      string `json:"poststed"`
	} `json:"forretningsadresse"`
	AntallAnsatte                     *int   `json:"antallAnsatte"`
	RegistreringsdatoEnhetsregisteret string `json:"registreringsdatoEnhetsregisteret"`
	Hjemmeside                        string `json:"hjemmeside"`
	Konkurs                           bool   `json:"konkurs"`
	UnderAvvikling                    bool   `json:"underAvvikling"`
}

func (e rawEnhet) toCompany() Company {
	c := Company{
		Organisasjonsnummer: e.Organisasjonsnummer,
		Navn:                e.Navn,
		Organisasjonsform:   e.Organisasjonsform.Kode,
		AntallAnsatte:       e.AntallAnsatte,
		Registreringsdato:   e.RegistreringsdatoEnhetsregisteret,
		Hjemmeside:          e.Hjemmeside,
		Konkurs:             e.Konkurs,
		UnderAvvikling:      e.UnderAvvikling,
	}
	if e.Naeringskode1 != nil {
		c.Naeringskode = e.Naeringskode1.Kode
		c.NaeringBeskrivelse = e.Naeringskode1.Beskrivelse
	}
	if e.Forretningsadresse != nil {
		c.Kommunenummer = e.Forretningsadresse.Kommunenummer
		c.Poststed = e.Forretningsadresse.Poststed
	}
	return c
}

// Search runs the filtered /enheter query and returns one page of COMPANY
// records. Limits are validated up front (typed errors), so an unsatisfiable
// filter never reaches the API as an opaque 400.
func (c *Client) Search(ctx context.Context, filter SearchFilter) (*SearchPage, error) {
	if err := filter.Validate(); err != nil {
		return nil, err
	}
	endpoint := c.baseURL + "/enheter?" + filter.query().Encode()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, fmt.Errorf("brreg: build request: %w", err)
	}
	req.Header.Set("Accept", "application/json")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("brreg: search request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("brreg: search returned %d", resp.StatusCode)
	}

	var raw rawSearchResponse
	if err := json.NewDecoder(resp.Body).Decode(&raw); err != nil {
		return nil, fmt.Errorf("brreg: decode search response: %w", err)
	}

	companies := make([]Company, 0, len(raw.Embedded.Enheter))
	for _, e := range raw.Embedded.Enheter {
		companies = append(companies, e.toCompany())
	}
	return &SearchPage{
		Companies:     companies,
		Page:          raw.Page.Number,
		Size:          raw.Page.Size,
		TotalElements: raw.Page.TotalElements,
		TotalPages:    raw.Page.TotalPages,
	}, nil
}
