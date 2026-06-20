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

// TestCompanyStructHasNoPIIFields statically asserts the Company record exposes
// no person/role/contact/birth-number field — a structural no-PII guarantee.
func TestCompanyStructHasNoPIIFields(t *testing.T) {
	forbidden := []string{"roller", "rolle", "person", "fodsel", "foedsel", "fnr", "birth", "kontakt", "epost", "telefon", "email", "phone"}
	rt := reflect.TypeFor[Company]()
	for i := 0; i < rt.NumField(); i++ {
		tag := strings.ToLower(rt.Field(i).Tag.Get("json"))
		name := strings.ToLower(rt.Field(i).Name)
		for _, f := range forbidden {
			if strings.Contains(tag, f) || strings.Contains(name, f) {
				t.Errorf("Company field %q (json:%q) matches forbidden PII token %q", rt.Field(i).Name, tag, f)
			}
		}
	}
}
