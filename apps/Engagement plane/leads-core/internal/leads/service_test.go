package leads

import (
	"context"
	"encoding/csv"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/brreg"
)

type fakeRepo struct {
	list    *SavedList
	created *CreateListInput
}

func (f *fakeRepo) CreateList(_ context.Context, in CreateListInput) (*SavedList, error) {
	f.created = &in
	return &SavedList{ID: "list_1", OrgID: in.OrgID, Name: in.Name, CompanyCount: len(in.Companies)}, nil
}
func (f *fakeRepo) ListLists(_ context.Context, _ string) ([]SavedList, error) { return nil, nil }
func (f *fakeRepo) GetList(_ context.Context, _, _ string) (*SavedList, error) { return f.list, nil }
func (f *fakeRepo) DeleteList(_ context.Context, _, _ string) error            { return nil }

func intPtr(v int) *int { return &v }

func TestCreateListRejectsInvalidInput(t *testing.T) {
	svc := NewService(&fakeRepo{}, nil)
	company := []brreg.Company{{Organisasjonsnummer: "1", Navn: "X"}}

	cases := []struct {
		name string
		in   CreateListInput
	}{
		{"empty org", CreateListInput{Name: "L", Companies: company}},
		{"empty name", CreateListInput{OrgID: "org-1", Companies: company}},
		{"no companies", CreateListInput{OrgID: "org-1", Name: "L"}},
	}
	for _, tc := range cases {
		if _, err := svc.CreateList(context.Background(), tc.in); err == nil {
			t.Errorf("%s: expected ErrInvalidInput, got nil", tc.name)
		}
	}
}

func TestCreateListPassesCompaniesThrough(t *testing.T) {
	repo := &fakeRepo{}
	svc := NewService(repo, nil)
	_, err := svc.CreateList(context.Background(), CreateListInput{
		OrgID:     "org-1",
		Name:      "Norwegian fish processors",
		Companies: []brreg.Company{{Organisasjonsnummer: "923609016", Navn: "AQUATIQ AS"}},
	})
	if err != nil {
		t.Fatalf("CreateList error: %v", err)
	}
	if repo.created == nil || repo.created.OrgID != "org-1" || len(repo.created.Companies) != 1 {
		t.Errorf("company not passed through to repo: %+v", repo.created)
	}
}

func TestExportCSVIsCompanyOnly(t *testing.T) {
	repo := &fakeRepo{list: &SavedList{
		ID:    "list_1",
		OrgID: "org-1",
		Name:  "Test",
		Companies: []brreg.Company{{
			Organisasjonsnummer: "923609016",
			Navn:                "AQUATIQ AS",
			Organisasjonsform:   "AS",
			Naeringskode:        "10.209",
			NaeringBeskrivelse:  "Bearbeiding av fisk",
			Kommunenummer:       "4601",
			Poststed:            "BERGEN",
			AntallAnsatte:       intPtr(42),
			Registreringsdato:   "1995-08-09",
			Hjemmeside:          "coresystem.com",
		}},
	}}
	svc := NewService(repo, nil)

	csvBytes, list, err := svc.ExportCSV(context.Background(), "org-1", "list_1", "user-1")
	if err != nil {
		t.Fatalf("ExportCSV error: %v", err)
	}
	if list.ID != "list_1" {
		t.Errorf("returned wrong list: %+v", list)
	}

	records, err := csv.NewReader(strings.NewReader(string(csvBytes))).ReadAll()
	if err != nil {
		t.Fatalf("CSV not parseable: %v", err)
	}
	if len(records) != 2 {
		t.Fatalf("got %d CSV rows (incl header), want 2", len(records))
	}
	// Header is the fixed company-only schema.
	if strings.Join(records[0], ",") != strings.Join(csvHeader, ",") {
		t.Errorf("CSV header = %v, want %v", records[0], csvHeader)
	}
	if records[1][0] != "923609016" || records[1][1] != "AQUATIQ AS" || records[1][7] != "42" {
		t.Errorf("company row wrong: %v", records[1])
	}

	// The whole CSV must contain NO person/role/contact/birth-number data.
	lower := strings.ToLower(string(csvBytes))
	for _, forbidden := range []string{"fodselsnummer", "roller", "person", "kontaktperson", "epost", "telefon", "fnr"} {
		if strings.Contains(lower, forbidden) {
			t.Errorf("CSV leaked forbidden PII token %q:\n%s", forbidden, csvBytes)
		}
	}
}

func TestSanitizeCSVFieldNeutralizesFormulaInjection(t *testing.T) {
	cases := []struct {
		name string
		in   string
		want string
	}{
		{"formula equals", "=1+1", "'=1+1"},
		{"formula plus", "+cmd", "'+cmd"},
		{"formula minus", "-2+3", "'-2+3"},
		{"formula at", "@SUM(A1)", "'@SUM(A1)"},
		{"leading tab", "\tvalue", "'\tvalue"},
		{"safe name", "AQUATIQ AS", "AQUATIQ AS"},
		{"empty", "", ""},
		{"org number", "923609016", "923609016"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := sanitizeCSVField(tc.in); got != tc.want {
				t.Fatalf("sanitizeCSVField(%q) = %q, want %q", tc.in, got, tc.want)
			}
		})
	}
}

func TestExportCSVNeutralizesFormulaInjectionInCompanyName(t *testing.T) {
	repo := &fakeRepo{list: &SavedList{
		ID:    "list_1",
		OrgID: "org-1",
		Name:  "Test",
		Companies: []brreg.Company{{
			Organisasjonsnummer: "923609016",
			Navn:                `=HYPERLINK("http://evil")`,
			Organisasjonsform:   "AS",
		}},
	}}
	svc := NewService(repo, nil)

	csvBytes, _, err := svc.ExportCSV(context.Background(), "org-1", "list_1", "user-1")
	if err != nil {
		t.Fatalf("ExportCSV error: %v", err)
	}
	records, err := csv.NewReader(strings.NewReader(string(csvBytes))).ReadAll()
	if err != nil {
		t.Fatalf("CSV not parseable: %v", err)
	}
	if got, want := records[1][1], `'=HYPERLINK("http://evil")`; got != want {
		t.Fatalf("company name not neutralized: got %q, want %q", got, want)
	}
}

// brregStub returns an httptest server that serves a single /enheter page with
// the given companies, salted with person-ish fields the endpoint never returns,
// plus /underenheter branches keyed by parent orgnr.
func brregStub(t *testing.T) *httptest.Server {
	t.Helper()
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		switch r.URL.Path {
		case "/enheter":
			_, _ = w.Write([]byte(`{
              "_embedded": { "enheter": [
                { "organisasjonsnummer": "923609016", "navn": "AQUATIQ AS",
                  "organisasjonsform": { "kode": "AS" },
                  "roller": [{ "person": { "fodselsnummer": "01017012345", "navn": "Ola Nordmann" } }] },
                { "organisasjonsnummer": "111111111", "navn": "BETA AS",
                  "organisasjonsform": { "kode": "AS" } }
              ]},
              "page": { "number": 0, "size": 20, "totalPages": 1, "totalElements": 2 }
            }`))
		case "/underenheter":
			parent := r.URL.Query().Get("overordnetEnhet")
			if parent == "923609016" {
				// One branch, plus a DUPLICATE of the parent's own orgnr to prove
				// dedupe collapses cross-list overlap.
				_, _ = w.Write([]byte(`{ "_embedded": { "underenheter": [
                    { "organisasjonsnummer": "929432827", "navn": "AQUATIQ BRANCH",
                      "organisasjonsform": { "kode": "BEDR" }, "overordnetEnhet": "923609016",
                      "kontaktperson": { "fodselsnummer": "02028023456" } },
                    { "organisasjonsnummer": "923609016", "navn": "AQUATIQ AS (dup of parent)",
                      "organisasjonsform": { "kode": "AS" }, "overordnetEnhet": "923609016" }
                ]}}`))
				return
			}
			_, _ = w.Write([]byte(`{ "_embedded": { "underenheter": [] }}`))
		default:
			t.Errorf("unexpected path %q", r.URL.Path)
			w.WriteHeader(http.StatusNotFound)
		}
	}))
}

func TestBuildListSearchesDedupesAndSaves(t *testing.T) {
	srv := brregStub(t)
	defer srv.Close()
	repo := &fakeRepo{}
	svc := NewService(repo, brreg.NewClientWithBaseURL(srv.URL))

	_, err := svc.BuildList(context.Background(), BuildListInput{
		OrgID:           "org-1",
		Name:            "Fish processors",
		CreatedBy:       "user-1",
		IncludeBranches: true,
		Filter:          brreg.SearchFilter{Naeringskode: "10.209"},
	})
	if err != nil {
		t.Fatalf("BuildList error: %v", err)
	}
	if repo.created == nil {
		t.Fatal("BuildList did not persist a list")
	}
	if repo.created.OrgID != "org-1" || repo.created.CreatedBy != "user-1" {
		t.Errorf("persisted org/creator wrong: %+v", repo.created)
	}
	// 2 search hits + 1 unique branch; the branch list's duplicate of the parent
	// orgnr (923609016) is collapsed by the orgnr-canonical dedupe.
	got := orgnrs(repo.created.Companies)
	want := []string{"923609016", "111111111", "929432827"}
	if strings.Join(got, ",") != strings.Join(want, ",") {
		t.Errorf("built list orgnrs = %v, want %v (dedup by orgnr, stable order)", got, want)
	}
}

// TestBuildListCompanyOnlyInvariant is the company-only invariant for the
// build_list action: the persisted list — even when upstream Brreg fixtures are
// salted with roller/person/fødselsnummer fields — must contain NO PII anywhere
// in its serialized form.
func TestBuildListCompanyOnlyInvariant(t *testing.T) {
	srv := brregStub(t)
	defer srv.Close()
	repo := &fakeRepo{}
	svc := NewService(repo, brreg.NewClientWithBaseURL(srv.URL))

	if _, err := svc.BuildList(context.Background(), BuildListInput{
		OrgID:           "org-1",
		Name:            "All",
		IncludeBranches: true,
		Filter:          brreg.SearchFilter{Naeringskode: "10.209"},
	}); err != nil {
		t.Fatalf("BuildList error: %v", err)
	}
	if repo.created == nil {
		t.Fatal("BuildList did not persist a list")
	}
	blob, err := json.Marshal(repo.created.Companies)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	lower := strings.ToLower(string(blob))
	for _, forbidden := range []string{
		"fodselsnummer", "fødselsnummer", "roller", "rolle", "person",
		"kontaktperson", "epost", "telefon", "fnr",
		"01017012345", "02028023456", "ola nordmann",
	} {
		if strings.Contains(lower, forbidden) {
			t.Errorf("built list leaked forbidden PII token %q:\n%s", forbidden, blob)
		}
	}
}

// TestBuildListIsIDORClean is the cross-tenant regression: the saved list is
// always scoped to the server-resolved OrgID passed by the caller, and there is
// no input path (search filter or otherwise) by which a client/model could
// redirect the write to another tenant. Two builds under different orgs never
// cross-contaminate.
func TestBuildListIsIDORClean(t *testing.T) {
	srv := brregStub(t)
	defer srv.Close()

	repoA := &fakeRepo{}
	if _, err := NewService(repoA, brreg.NewClientWithBaseURL(srv.URL)).BuildList(context.Background(), BuildListInput{
		OrgID:  "org-A",
		Name:   "A list",
		Filter: brreg.SearchFilter{Naeringskode: "10.209"},
	}); err != nil {
		t.Fatalf("BuildList(org-A) error: %v", err)
	}
	repoB := &fakeRepo{}
	if _, err := NewService(repoB, brreg.NewClientWithBaseURL(srv.URL)).BuildList(context.Background(), BuildListInput{
		OrgID:  "org-B",
		Name:   "B list",
		Filter: brreg.SearchFilter{Naeringskode: "10.209"},
	}); err != nil {
		t.Fatalf("BuildList(org-B) error: %v", err)
	}

	if repoA.created.OrgID != "org-A" {
		t.Errorf("org-A build persisted to org %q, want org-A", repoA.created.OrgID)
	}
	if repoB.created.OrgID != "org-B" {
		t.Errorf("org-B build persisted to org %q, want org-B", repoB.created.OrgID)
	}
}

func TestBuildListRejectsMissingOrgOrName(t *testing.T) {
	svc := NewService(&fakeRepo{}, brreg.NewClient())
	cases := []BuildListInput{
		{Name: "L", Filter: brreg.SearchFilter{Naeringskode: "10"}},      // no org
		{OrgID: "org-1", Filter: brreg.SearchFilter{Naeringskode: "10"}}, // no name
	}
	for i, in := range cases {
		if _, err := svc.BuildList(context.Background(), in); err == nil {
			t.Errorf("case %d: expected ErrInvalidInput, got nil", i)
		}
	}
}

func orgnrs(cs []brreg.Company) []string {
	out := make([]string, 0, len(cs))
	for _, c := range cs {
		out = append(out, c.Organisasjonsnummer)
	}
	return out
}
