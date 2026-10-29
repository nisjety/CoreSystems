// Package brreg is a filtered client for the open Enhetsregisteret API
// (Brønnøysundregistrene), purpose-built for company lead-building.
//
// Unlike org-core's navn-only, size-20 lookup client, this client filters by
// næringskode / kommunenummer / organisasjonsform / employee-range /
// registration-date and paginates within the API's hard limits. It enriches a
// company with its sub-entities/branches (`/underenheter`) and its filed annual
// accounts (`/regnskap`, Regnskapsregisteret), and de-duplicates merged result
// sets on the canonical organisasjonsnummer.
//
// COMPANY DATA ONLY: it calls only `/enheter`, `/underenheter` (both company
// records) and `/regnskap` (aggregate company financials). It NEVER calls
// `/enheter/{orgnr}/roller`, so it never fetches a person, a role, or a
// fødselsnummer (birth number). Every record this package returns ([Company],
// [Branch], [Financials]) carries no person/role/PII field — see the
// company-only invariant test in client_test.go.
//
// Docs: https://data.brreg.no/enhetsregisteret/api/docs/index.html
//
//	Regnskapsregisteret: https://data.brreg.no/regnskapsregisteret/regnskap/{orgnr}
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

// DefaultRegnskapBaseURL is the public Regnskapsregisteret root (no API key).
// The financials endpoint is `/regnskap/{orgnr}` under this base.
const DefaultRegnskapBaseURL = "https://data.brreg.no/regnskapsregisteret"

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

// Client is a lightweight HTTP client for the filtered Enhetsregisteret search
// (`/enheter`, `/underenheter`) and the Regnskapsregisteret financials endpoint
// (`/regnskap`).
type Client struct {
	baseURL         string
	regnskapBaseURL string
	httpClient      *http.Client
}

func NewClient() *Client {
	return NewClientWithBaseURLs(DefaultBaseURL, DefaultRegnskapBaseURL)
}

// NewClientWithBaseURL points the Enhetsregisteret base at baseURL and keeps the
// public Regnskapsregisteret base. Retained for callers that only exercise the
// /enheter + /underenheter surface.
func NewClientWithBaseURL(baseURL string) *Client {
	return NewClientWithBaseURLs(baseURL, DefaultRegnskapBaseURL)
}

// NewClientWithBaseURLs allows tests to point both the Enhetsregisteret base and
// the Regnskapsregisteret base at httptest servers.
func NewClientWithBaseURLs(baseURL, regnskapBaseURL string) *Client {
	return &Client{
		baseURL:         strings.TrimRight(strings.TrimSpace(baseURL), "/"),
		regnskapBaseURL: strings.TrimRight(strings.TrimSpace(regnskapBaseURL), "/"),
		httpClient:      &http.Client{Timeout: 15 * time.Second},
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

// getJSON GETs an absolute URL with an Accept: application/json header and
// decodes the body into dst. Non-2xx is returned as a typed-free error so the
// caller can map it. Used by the /underenheter and /regnskap reads.
func (c *Client) getJSON(ctx context.Context, rawURL string, dst any) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, rawURL, nil)
	if err != nil {
		return fmt.Errorf("brreg: build request: %w", err)
	}
	req.Header.Set("Accept", "application/json")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("brreg: request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("brreg: %s returned %d", rawURL, resp.StatusCode)
	}
	if err := json.NewDecoder(resp.Body).Decode(dst); err != nil {
		return fmt.Errorf("brreg: decode response: %w", err)
	}
	return nil
}

// ---------------------------------------------------------------------------
// Sub-entities / branches (/underenheter) — COMPANY DATA ONLY.
// ---------------------------------------------------------------------------

// Branch is a COMPANY-ONLY sub-entity (underenhet) record: a branch/site of a
// parent company. Like [Company] it carries no person/role/birth-number field.
// `OverordnetEnhet` is the parent company's organisasjonsnummer — itself a
// company identifier, never a person.
type Branch struct {
	Organisasjonsnummer string `json:"organisasjonsnummer"`
	Navn                string `json:"navn"`
	Organisasjonsform   string `json:"organisasjonsform,omitempty"`
	Naeringskode        string `json:"naeringskode,omitempty"`
	NaeringBeskrivelse  string `json:"naering_beskrivelse,omitempty"`
	Kommunenummer       string `json:"kommunenummer,omitempty"`
	Poststed            string `json:"poststed,omitempty"`
	AntallAnsatte       *int   `json:"antall_ansatte,omitempty"`
	Registreringsdato   string `json:"registreringsdato,omitempty"`
	OverordnetEnhet     string `json:"overordnet_enhet,omitempty"`
}

// rawUnderenheterResponse mirrors the parts of the /underenheter list we map.
type rawUnderenheterResponse struct {
	Embedded struct {
		Underenheter []rawUnderenhet `json:"underenheter"`
	} `json:"_embedded"`
}

// rawUnderenhet mirrors only the COMPANY fields of a sub-entity — never
// roller/persons. Sub-entities use beliggenhetsadresse (location address)
// rather than forretningsadresse.
type rawUnderenhet struct {
	Organisasjonsnummer string `json:"organisasjonsnummer"`
	Navn                string `json:"navn"`
	Organisasjonsform   struct {
		Kode string `json:"kode"`
	} `json:"organisasjonsform"`
	Naeringskode1 *struct {
		Kode        string `json:"kode"`
		Beskrivelse string `json:"beskrivelse"`
	} `json:"naeringskode1"`
	Beliggenhetsadresse *struct {
		Kommunenummer string `json:"kommunenummer"`
		Poststed      string `json:"poststed"`
	} `json:"beliggenhetsadresse"`
	AntallAnsatte                     *int   `json:"antallAnsatte"`
	RegistreringsdatoEnhetsregisteret string `json:"registreringsdatoEnhetsregisteret"`
	OverordnetEnhet                   string `json:"overordnetEnhet"`
}

func (u rawUnderenhet) toBranch() Branch {
	b := Branch{
		Organisasjonsnummer: u.Organisasjonsnummer,
		Navn:                u.Navn,
		Organisasjonsform:   u.Organisasjonsform.Kode,
		AntallAnsatte:       u.AntallAnsatte,
		Registreringsdato:   u.RegistreringsdatoEnhetsregisteret,
		OverordnetEnhet:     u.OverordnetEnhet,
	}
	if u.Naeringskode1 != nil {
		b.Naeringskode = u.Naeringskode1.Kode
		b.NaeringBeskrivelse = u.Naeringskode1.Beskrivelse
	}
	if u.Beliggenhetsadresse != nil {
		b.Kommunenummer = u.Beliggenhetsadresse.Kommunenummer
		b.Poststed = u.Beliggenhetsadresse.Poststed
	}
	return b
}

// Branches returns the sub-entities/branches whose parent (overordnetEnhet) is
// orgnr, via `/underenheter?overordnetEnhet={orgnr}`. COMPANY DATA ONLY.
func (c *Client) Branches(ctx context.Context, orgnr string) ([]Branch, error) {
	orgnr = strings.TrimSpace(orgnr)
	if !isOrgNumber(orgnr) {
		return nil, fmt.Errorf("brreg: branches requires a 9-digit organisasjonsnummer, got %q", orgnr)
	}
	q := url.Values{}
	q.Set("overordnetEnhet", orgnr)
	q.Set("size", strconv.Itoa(maxSize))
	endpoint := c.baseURL + "/underenheter?" + q.Encode()

	var raw rawUnderenheterResponse
	if err := c.getJSON(ctx, endpoint, &raw); err != nil {
		return nil, err
	}
	branches := make([]Branch, 0, len(raw.Embedded.Underenheter))
	for _, u := range raw.Embedded.Underenheter {
		branches = append(branches, u.toBranch())
	}
	return branches, nil
}

// ---------------------------------------------------------------------------
// Financials (/regnskap, Regnskapsregisteret) — AGGREGATE COMPANY DATA ONLY.
// ---------------------------------------------------------------------------

// Financials is one filed annual account (årsregnskap) for a company —
// aggregate figures only. It carries no person/role/birth-number field: the
// only identifier is the company's own organisasjonsnummer.
type Financials struct {
	Organisasjonsnummer string `json:"organisasjonsnummer"`
	Regnskapstype       string `json:"regnskapstype,omitempty"`
	FraDato             string `json:"fra_dato,omitempty"`
	TilDato             string `json:"til_dato,omitempty"`
	Valuta              string `json:"valuta,omitempty"`
	SumDriftsinntekter  *int64 `json:"sum_driftsinntekter,omitempty"`
	Driftsresultat      *int64 `json:"driftsresultat,omitempty"`
	Aarsresultat        *int64 `json:"aarsresultat,omitempty"`
	SumEiendeler        *int64 `json:"sum_eiendeler,omitempty"`
	SumEgenkapital      *int64 `json:"sum_egenkapital,omitempty"`
	SumGjeld            *int64 `json:"sum_gjeld,omitempty"`
}

// rawRegnskap mirrors only the aggregate figures we surface from a
// Regnskapsregisteret annual account. There are no person fields in this
// response at all; we map an explicit allowlist of aggregate totals.
type rawRegnskap struct {
	Regnskapstype string `json:"regnskapstype"`
	Virksomhet    struct {
		Organisasjonsnummer string `json:"organisasjonsnummer"`
	} `json:"virksomhet"`
	Regnskapsperiode struct {
		FraDato string `json:"fraDato"`
		TilDato string `json:"tilDato"`
	} `json:"regnskapsperiode"`
	Valuta                   string `json:"valuta"`
	ResultatregnskapResultat struct {
		Aarsresultat   *int64 `json:"aarsresultat"`
		Driftsresultat struct {
			Driftsresultat  *int64 `json:"driftsresultat"`
			Driftsinntekter struct {
				SumDriftsinntekter *int64 `json:"sumDriftsinntekter"`
			} `json:"driftsinntekter"`
		} `json:"driftsresultat"`
	} `json:"resultatregnskapResultat"`
	Eiendeler struct {
		SumEiendeler *int64 `json:"sumEiendeler"`
	} `json:"eiendeler"`
	EgenkapitalGjeld struct {
		Egenkapital struct {
			SumEgenkapital *int64 `json:"sumEgenkapital"`
		} `json:"egenkapital"`
		GjeldOversikt struct {
			SumGjeld *int64 `json:"sumGjeld"`
		} `json:"gjeldOversikt"`
	} `json:"egenkapitalGjeld"`
}

func (r rawRegnskap) toFinancials(orgnr string) Financials {
	on := strings.TrimSpace(r.Virksomhet.Organisasjonsnummer)
	if on == "" {
		on = orgnr
	}
	return Financials{
		Organisasjonsnummer: on,
		Regnskapstype:       r.Regnskapstype,
		FraDato:             r.Regnskapsperiode.FraDato,
		TilDato:             r.Regnskapsperiode.TilDato,
		Valuta:              r.Valuta,
		SumDriftsinntekter:  r.ResultatregnskapResultat.Driftsresultat.Driftsinntekter.SumDriftsinntekter,
		Driftsresultat:      r.ResultatregnskapResultat.Driftsresultat.Driftsresultat,
		Aarsresultat:        r.ResultatregnskapResultat.Aarsresultat,
		SumEiendeler:        r.Eiendeler.SumEiendeler,
		SumEgenkapital:      r.EgenkapitalGjeld.Egenkapital.SumEgenkapital,
		SumGjeld:            r.EgenkapitalGjeld.GjeldOversikt.SumGjeld,
	}
}

// Financials returns the company's filed annual accounts from
// Regnskapsregisteret (`/regnskap/{orgnr}`), newest first as the API returns
// them. AGGREGATE COMPANY FIGURES ONLY — never roller/persons. A 404 (no filed
// accounts) is reported as an empty slice, not an error.
func (c *Client) Financials(ctx context.Context, orgnr string) ([]Financials, error) {
	orgnr = strings.TrimSpace(orgnr)
	if !isOrgNumber(orgnr) {
		return nil, fmt.Errorf("brreg: financials requires a 9-digit organisasjonsnummer, got %q", orgnr)
	}
	endpoint := c.regnskapBaseURL + "/regnskap/" + orgnr

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, fmt.Errorf("brreg: build request: %w", err)
	}
	req.Header.Set("Accept", "application/json")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("brreg: regnskap request: %w", err)
	}
	defer resp.Body.Close()

	// No filed accounts is a normal, non-error outcome for many companies.
	if resp.StatusCode == http.StatusNotFound {
		return []Financials{}, nil
	}
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("brreg: regnskap returned %d", resp.StatusCode)
	}

	var raw []rawRegnskap
	if err := json.NewDecoder(resp.Body).Decode(&raw); err != nil {
		return nil, fmt.Errorf("brreg: decode regnskap response: %w", err)
	}
	out := make([]Financials, 0, len(raw))
	for _, r := range raw {
		out = append(out, r.toFinancials(orgnr))
	}
	return out, nil
}

// ---------------------------------------------------------------------------
// Cross-list dedupe (orgnr canonical).
// ---------------------------------------------------------------------------

// DedupeCompanies merges any number of company result sets into one slice with
// at most one record per canonical organisasjonsnummer (whitespace-trimmed).
// The FIRST occurrence wins, and stable input order is preserved — so a primary
// search list keeps priority over later enrichment lists. Records with an empty
// orgnr are dropped (they cannot be a unique lead).
func DedupeCompanies(lists ...[]Company) []Company {
	seen := make(map[string]struct{})
	out := make([]Company, 0)
	for _, list := range lists {
		for _, c := range list {
			key := canonicalOrgnr(c.Organisasjonsnummer)
			if key == "" {
				continue
			}
			if _, dup := seen[key]; dup {
				continue
			}
			seen[key] = struct{}{}
			out = append(out, c)
		}
	}
	return out
}

// canonicalOrgnr is the dedupe key: the organisasjonsnummer with surrounding
// whitespace removed. The orgnr is the company's canonical identifier.
func canonicalOrgnr(orgnr string) string { return strings.TrimSpace(orgnr) }

// isOrgNumber reports whether s is exactly 9 ASCII digits (a Norwegian
// organisasjonsnummer).
func isOrgNumber(s string) bool {
	if len(s) != 9 {
		return false
	}
	for i := 0; i < len(s); i++ {
		if s[i] < '0' || s[i] > '9' {
			return false
		}
	}
	return true
}
