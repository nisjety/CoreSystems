package brreg

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"testing"
)

func intPtr(v int) *int { return &v }

func TestQueryBuildsSupportedFacets(t *testing.T) {
	q := SearchFilter{
		Naeringskode:         "10.20",
		Kommunenummer:        "4601",
		Organisasjonsform:    "AS",
		FraAntallAnsatte:     intPtr(5),
		TilAntallAnsatte:     intPtr(50),
		FraRegistreringsdato: "2020-01-01",
		TilRegistreringsdato: "2024-12-31",
		Page:                 2,
		Size:                 25,
	}.query()

	cases := map[string]string{
		"naeringskode":                         "10.20",
		"kommunenummer":                        "4601",
		"organisasjonsform":                    "AS",
		"fraAntallAnsatte":                     "5",
		"tilAntallAnsatte":                     "50",
		"fraRegistreringsdatoEnhetsregisteret": "2020-01-01",
		"tilRegistreringsdatoEnhetsregisteret": "2024-12-31",
		"size":                                 "25",
		"page":                                 "2",
	}
	for k, want := range cases {
		if got := q.Get(k); got != want {
			t.Errorf("query[%s] = %q, want %q", k, got, want)
		}
	}
}

func TestQueryOmitsEmptyFacetsAndPageZero(t *testing.T) {
	q := SearchFilter{Naeringskode: "47.11"}.query()
	if q.Has("kommunenummer") || q.Has("organisasjonsform") || q.Has("page") || q.Has("fraAntallAnsatte") {
		t.Errorf("empty facets / page 0 must be omitted, got %v", q)
	}
	if q.Get("size") != "20" {
		t.Errorf("default size = %q, want 20", q.Get("size"))
	}
}

func TestValidateRejectsEmployeeBand1to4(t *testing.T) {
	for _, v := range []int{1, 2, 3, 4} {
		if err := (SearchFilter{FraAntallAnsatte: intPtr(v)}).Validate(); err != ErrEmployeeBandUnsupported {
			t.Errorf("fraAntallAnsatte=%d Validate() = %v, want ErrEmployeeBandUnsupported", v, err)
		}
		if err := (SearchFilter{TilAntallAnsatte: intPtr(v)}).Validate(); err != ErrEmployeeBandUnsupported {
			t.Errorf("tilAntallAnsatte=%d Validate() = %v, want ErrEmployeeBandUnsupported", v, err)
		}
	}
	// 0 and >=5 are allowed.
	for _, v := range []int{0, 5, 100} {
		if err := (SearchFilter{FraAntallAnsatte: intPtr(v)}).Validate(); err != nil {
			t.Errorf("fraAntallAnsatte=%d Validate() = %v, want nil", v, err)
		}
	}
}

func TestValidateRejectsDeepPaging(t *testing.T) {
	// (page+1)*size must not exceed 10000.
	if err := (SearchFilter{Page: 500, Size: 20}).Validate(); err != ErrDeepPagingLimit {
		t.Errorf("Validate() = %v, want ErrDeepPagingLimit (501*20 > 10000)", err)
	}
	// Exactly at the window is allowed: 500*20 = 10000.
	if err := (SearchFilter{Page: 499, Size: 20}).Validate(); err != nil {
		t.Errorf("Validate() at the window = %v, want nil", err)
	}
}

func TestSearchParsesCompanyFieldsOnly(t *testing.T) {
	// Fixture includes person-ish fields (roller, epostadresse) the API never
	// returns for /enheter — proving the client maps COMPANY fields only.
	fixture := `{
      "_embedded": { "enheter": [
        {
          "organisasjonsnummer": "923609016",
          "navn": "AQUATIQ AS",
          "organisasjonsform": { "kode": "AS", "beskrivelse": "Aksjeselskap" },
          "naeringskode1": { "kode": "10.209", "beskrivelse": "Bearbeiding av fisk" },
          "forretningsadresse": { "kommunenummer": "4601", "poststed": "BERGEN" },
          "antallAnsatte": 42,
          "registreringsdatoEnhetsregisteret": "1995-08-09",
          "hjemmeside": "aquatiq.com",
          "konkurs": false,
          "underAvvikling": false,
          "epostadresse": "should-not-be-mapped@example.com",
          "telefon": "55000000",
          "roller": [{ "person": { "fodselsnummer": "01017012345", "navn": "Ola Nordmann" } }]
        }
      ]},
      "page": { "number": 0, "size": 20, "totalPages": 1, "totalElements": 1 }
    }`

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/enheter" {
			t.Errorf("unexpected path %q", r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(fixture))
	}))
	defer srv.Close()

	page, err := NewClientWithBaseURL(srv.URL).Search(context.Background(), SearchFilter{Naeringskode: "10.209"})
	if err != nil {
		t.Fatalf("Search error: %v", err)
	}
	if len(page.Companies) != 1 {
		t.Fatalf("got %d companies, want 1", len(page.Companies))
	}
	c := page.Companies[0]
	if c.Organisasjonsnummer != "923609016" || c.Navn != "AQUATIQ AS" || c.Organisasjonsform != "AS" {
		t.Errorf("company core fields wrong: %+v", c)
	}
	if c.Naeringskode != "10.209" || c.Kommunenummer != "4601" || c.Poststed != "BERGEN" {
		t.Errorf("company facet fields wrong: %+v", c)
	}
	if c.AntallAnsatte == nil || *c.AntallAnsatte != 42 {
		t.Errorf("antallAnsatte wrong: %+v", c.AntallAnsatte)
	}

	// The serialized company must carry NONE of the person/role/contact data
	// that appeared in the upstream fixture.
	blob, _ := json.Marshal(c)
	for _, forbidden := range []string{"fodselsnummer", "Ola Nordmann", "roller", "epostadresse", "should-not-be-mapped", "telefon", "55000000"} {
		if strings.Contains(string(blob), forbidden) {
			t.Errorf("serialized Company leaked %q: %s", forbidden, blob)
		}
	}
}

// forbiddenPIITokens are substrings that must never appear in a returned
// record's field names, JSON tags, or serialized output. The headline safety
// property: leads-core is company-data-only.
var forbiddenPIITokens = []string{
	"roller", "rolle", "person", "fodsel", "foedsel", "fødsel", "fnr",
	"birth", "kontakt", "epost", "telefon", "email", "phone", "navnperson",
}

// assertStructHasNoPIIFields statically asserts a record type exposes no
// person/role/contact/birth-number field — a structural no-PII guarantee.
func assertStructHasNoPIIFields(t *testing.T, rt reflect.Type) {
	t.Helper()
	for i := 0; i < rt.NumField(); i++ {
		tag := strings.ToLower(rt.Field(i).Tag.Get("json"))
		name := strings.ToLower(rt.Field(i).Name)
		for _, f := range forbiddenPIITokens {
			if strings.Contains(tag, f) || strings.Contains(name, f) {
				t.Errorf("%s field %q (json:%q) matches forbidden PII token %q", rt.Name(), rt.Field(i).Name, tag, f)
			}
		}
	}
}

// TestCompanyOnlyInvariant_NoPIIFieldsOnAnyRecord is the headline company-only
// invariant test: it asserts that NONE of the record types this package can
// return (Company, Branch, Financials) carries a person, role, or
// fødselsnummer (national ID) field. It is the structural guarantee that a lead
// record never holds PII.
func TestCompanyOnlyInvariant_NoPIIFieldsOnAnyRecord(t *testing.T) {
	assertStructHasNoPIIFields(t, reflect.TypeFor[Company]())
	assertStructHasNoPIIFields(t, reflect.TypeFor[Branch]())
	assertStructHasNoPIIFields(t, reflect.TypeFor[Financials]())
}

func TestBranchesParsesCompanyFieldsOnly(t *testing.T) {
	// Fixture includes person-ish fields (roller, kontaktperson) the
	// /underenheter endpoint never returns — proving the mapper drops them.
	fixture := `{
      "_embedded": { "underenheter": [
        {
          "organisasjonsnummer": "929432827",
          "navn": "AQUATIQ FOOD AUTOMATION",
          "organisasjonsform": { "kode": "BEDR", "beskrivelse": "Bedrift" },
          "naeringskode1": { "kode": "10.209", "beskrivelse": "Bearbeiding av fisk" },
          "beliggenhetsadresse": { "kommunenummer": "4601", "poststed": "BERGEN" },
          "antallAnsatte": 12,
          "registreringsdatoEnhetsregisteret": "2010-03-01",
          "overordnetEnhet": "923609016",
          "kontaktperson": { "navn": "Kari Nordmann", "fodselsnummer": "02028023456" },
          "roller": [{ "person": { "navn": "Per Hansen" } }]
        }
      ]}
    }`

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/underenheter" {
			t.Errorf("unexpected path %q", r.URL.Path)
		}
		if got := r.URL.Query().Get("overordnetEnhet"); got != "923609016" {
			t.Errorf("overordnetEnhet = %q, want 923609016", got)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(fixture))
	}))
	defer srv.Close()

	branches, err := NewClientWithBaseURL(srv.URL).Branches(context.Background(), "923609016")
	if err != nil {
		t.Fatalf("Branches error: %v", err)
	}
	if len(branches) != 1 {
		t.Fatalf("got %d branches, want 1", len(branches))
	}
	b := branches[0]
	if b.Organisasjonsnummer != "929432827" || b.Navn != "AQUATIQ FOOD AUTOMATION" {
		t.Errorf("branch core fields wrong: %+v", b)
	}
	if b.Naeringskode != "10.209" || b.Kommunenummer != "4601" || b.Poststed != "BERGEN" {
		t.Errorf("branch facet fields wrong: %+v", b)
	}
	if b.OverordnetEnhet != "923609016" {
		t.Errorf("overordnet_enhet = %q, want 923609016", b.OverordnetEnhet)
	}
	if b.AntallAnsatte == nil || *b.AntallAnsatte != 12 {
		t.Errorf("antallAnsatte wrong: %+v", b.AntallAnsatte)
	}

	assertSerializedHasNoPII(t, b, "Kari Nordmann", "Per Hansen", "02028023456")
}

func TestBranchesRejectsNonOrgNumber(t *testing.T) {
	c := NewClientWithBaseURL("http://unused.invalid")
	for _, bad := range []string{"", "12345678", "Equinor", "12345678a"} {
		if _, err := c.Branches(context.Background(), bad); err == nil {
			t.Errorf("Branches(%q) = nil error, want validation error", bad)
		}
	}
}

func TestFinancialsParsesAggregateFiguresOnly(t *testing.T) {
	// The /regnskap response is a JSON ARRAY of annual accounts. The fixture
	// also salts in person-ish fields the endpoint never returns, to prove the
	// allowlist mapper drops them.
	fixture := `[
      {
        "regnskapstype": "SELSKAP",
        "virksomhet": { "organisasjonsnummer": "923609016" },
        "regnskapsperiode": { "fraDato": "2024-01-01", "tilDato": "2024-12-31" },
        "valuta": "NOK",
        "resultatregnskapResultat": {
          "aarsresultat": 8141000,
          "driftsresultat": {
            "driftsresultat": 10347000,
            "driftsinntekter": { "sumDriftsinntekter": 72543000 }
          }
        },
        "eiendeler": { "sumEiendeler": 109150000 },
        "egenkapitalGjeld": {
          "egenkapital": { "sumEgenkapital": 41090000 },
          "gjeldOversikt": { "sumGjeld": 68060000 }
        },
        "revisor": { "navn": "Revisor Person", "fodselsnummer": "03038024567" }
      }
    ]`

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/regnskap/923609016" {
			t.Errorf("unexpected path %q", r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(fixture))
	}))
	defer srv.Close()

	fins, err := NewClientWithBaseURLs("http://unused.invalid", srv.URL).Financials(context.Background(), "923609016")
	if err != nil {
		t.Fatalf("Financials error: %v", err)
	}
	if len(fins) != 1 {
		t.Fatalf("got %d financial records, want 1", len(fins))
	}
	f := fins[0]
	if f.Organisasjonsnummer != "923609016" || f.Regnskapstype != "SELSKAP" || f.Valuta != "NOK" {
		t.Errorf("financials core fields wrong: %+v", f)
	}
	if f.FraDato != "2024-01-01" || f.TilDato != "2024-12-31" {
		t.Errorf("financials period wrong: %+v", f)
	}
	if f.Aarsresultat == nil || *f.Aarsresultat != 8141000 {
		t.Errorf("aarsresultat wrong: %+v", f.Aarsresultat)
	}
	if f.Driftsresultat == nil || *f.Driftsresultat != 10347000 {
		t.Errorf("driftsresultat wrong: %+v", f.Driftsresultat)
	}
	if f.SumDriftsinntekter == nil || *f.SumDriftsinntekter != 72543000 {
		t.Errorf("sumDriftsinntekter wrong: %+v", f.SumDriftsinntekter)
	}
	if f.SumEiendeler == nil || *f.SumEiendeler != 109150000 {
		t.Errorf("sumEiendeler wrong: %+v", f.SumEiendeler)
	}
	if f.SumEgenkapital == nil || *f.SumEgenkapital != 41090000 {
		t.Errorf("sumEgenkapital wrong: %+v", f.SumEgenkapital)
	}
	if f.SumGjeld == nil || *f.SumGjeld != 68060000 {
		t.Errorf("sumGjeld wrong: %+v", f.SumGjeld)
	}

	assertSerializedHasNoPII(t, f, "Revisor Person", "03038024567")
}

func TestFinancialsReturnsEmptyOn404(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNotFound)
	}))
	defer srv.Close()

	fins, err := NewClientWithBaseURLs("http://unused.invalid", srv.URL).Financials(context.Background(), "999888777")
	if err != nil {
		t.Fatalf("Financials on 404 = error %v, want nil error + empty slice", err)
	}
	if len(fins) != 0 {
		t.Errorf("Financials on 404 = %d records, want 0", len(fins))
	}
}

func TestDedupeCompaniesByOrgnr(t *testing.T) {
	primary := []Company{
		{Organisasjonsnummer: "923609016", Navn: "AQUATIQ AS"},
		{Organisasjonsnummer: "111111111", Navn: "FIRST WINS"},
	}
	enrichment := []Company{
		{Organisasjonsnummer: " 923609016 ", Navn: "AQUATIQ AS (dup, whitespace)"},
		{Organisasjonsnummer: "222222222", Navn: "SECOND LIST UNIQUE"},
		{Organisasjonsnummer: "", Navn: "NO ORGNR - DROPPED"},
		{Organisasjonsnummer: "222222222", Navn: "SECOND LIST DUP"},
	}

	merged := DedupeCompanies(primary, enrichment)

	if len(merged) != 3 {
		t.Fatalf("got %d merged companies, want 3: %+v", len(merged), merged)
	}
	// First occurrence wins, stable order preserved.
	wantOrder := []struct{ orgnr, navn string }{
		{"923609016", "AQUATIQ AS"},
		{"111111111", "FIRST WINS"},
		{"222222222", "SECOND LIST UNIQUE"},
	}
	for i, w := range wantOrder {
		if merged[i].Organisasjonsnummer != w.orgnr || merged[i].Navn != w.navn {
			t.Errorf("merged[%d] = {%q,%q}, want {%q,%q}", i, merged[i].Organisasjonsnummer, merged[i].Navn, w.orgnr, w.navn)
		}
	}
}

func TestDedupeCompaniesEmptyInputs(t *testing.T) {
	if got := DedupeCompanies(); len(got) != 0 {
		t.Errorf("DedupeCompanies() = %d, want 0", len(got))
	}
	if got := DedupeCompanies(nil, []Company{}); len(got) != 0 {
		t.Errorf("DedupeCompanies(nil, empty) = %d, want 0", len(got))
	}
}

// assertSerializedHasNoPII marshals v to JSON and fails if any forbidden PII
// token (or the supplied upstream-fixture person markers) leaks into the wire
// form.
func assertSerializedHasNoPII(t *testing.T, v any, extraForbidden ...string) {
	t.Helper()
	blob, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	lower := strings.ToLower(string(blob))
	for _, f := range forbiddenPIITokens {
		if strings.Contains(lower, f) {
			t.Errorf("serialized record leaked forbidden PII token %q: %s", f, blob)
		}
	}
	for _, f := range extraForbidden {
		if strings.Contains(string(blob), f) {
			t.Errorf("serialized record leaked upstream PII value %q: %s", f, blob)
		}
	}
}
