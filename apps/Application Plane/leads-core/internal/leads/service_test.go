package leads

import (
	"context"
	"encoding/csv"
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
func (f *fakeRepo) DeleteList(_ context.Context, _, _ string) error           { return nil }

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
			Hjemmeside:          "aquatiq.com",
		}},
	}}
	svc := NewService(repo, nil)

	csvBytes, list, err := svc.ExportCSV(context.Background(), "org-1", "list_1")
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
