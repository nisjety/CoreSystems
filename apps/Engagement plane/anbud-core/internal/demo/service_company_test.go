package demo

import (
	"context"
	"errors"
	"testing"

	"github.com/triodelab/anbud/internal/orgcore"
)

func TestCompanyReturnsWhatTheRegisterHolds(t *testing.T) {
	t.Parallel()

	employees := 550
	companies := &fakeCompanies{org: orgcore.Organisation{
		OrgNumber: "974760673", Name: "REGISTERENHETEN I BRØNNØYSUND", OrgForm: "ORGL", NACE: "84.110",
		NACEText: "Generell offentlig administrasjon", Municipality: "BRØNNØY", MunicipalityNumber: "1813", Employees: &employees,
	}}
	got, err := newService(t, &fakeNotices{}, companies).Company(context.Background(), "NO 974 760 673 MVA")
	if err != nil {
		t.Fatalf("Company: %v", err)
	}
	if len(companies.calls) != 1 || companies.calls[0] != "974760673" {
		t.Errorf("lookups = %v, want one with the normalised number", companies.calls)
	}
	if got.OrgNumber != "974760673" || got.Name != "REGISTERENHETEN I BRØNNØYSUND" || got.OrgForm != "ORGL" || got.NACE != "84.110" ||
		got.Municipality != "BRØNNØY" || got.MunicipalityNumber != "1813" || got.Employees == nil || *got.Employees != 550 || !got.CanBid {
		t.Errorf("company = %+v", got)
	}
}

func TestCompanyReportsTheRegisterFlags(t *testing.T) {
	t.Parallel()

	companies := &fakeCompanies{org: orgcore.Organisation{OrgNumber: "974760673", Bankrupt: true, UnderLiquidation: true, ForcedLiquidation: true}}
	got, err := newService(t, &fakeNotices{}, companies).Company(context.Background(), "974760673")
	if err != nil {
		t.Fatalf("Company: %v", err)
	}
	if got.CanBid || !got.Bankrupt || !got.UnderLiquidation || !got.ForcedLiquidation {
		t.Errorf("company = %+v", got)
	}
}

func TestCompanyMapsEveryWayItCanFail(t *testing.T) {
	t.Parallel()

	boom := errors.New("connection refused")
	for name, tc := range map[string]struct {
		companies CompanyLookup
		number    string
		want      error
	}{
		"not configured":      {nil, "974760673", ErrCompanyNotConfigured},
		"invalid number":      {&fakeCompanies{}, "974760674", ErrInvalidOrgNumber},
		"not a number":        {&fakeCompanies{}, "x", ErrInvalidOrgNumber},
		"not in the register": {&fakeCompanies{err: orgcore.ErrNotFound}, "974760673", ErrCompanyNotFound},
		"org-core is down":    {&fakeCompanies{err: boom}, "974760673", ErrCompanyUnavailable},
		"credential refused":  {&fakeCompanies{err: orgcore.ErrUnauthorized}, "974760673", ErrCompanyUnavailable},
		"scope not granted":   {&fakeCompanies{err: orgcore.ErrScopeDenied}, "974760673", ErrCompanyUnavailable},
	} {
		_, err := newService(t, &fakeNotices{}, tc.companies).Company(context.Background(), tc.number)
		if !errors.Is(err, tc.want) {
			t.Errorf("%s: err = %v, want %v", name, err, tc.want)
		}
	}

	// The cause is kept for the log, behind the error the caller sees.
	_, err := newService(t, &fakeNotices{}, &fakeCompanies{err: boom}).Company(context.Background(), "974760673")
	if !errors.Is(err, ErrCompanyUnavailable) || !errors.Is(err, boom) {
		t.Errorf("err = %v, want ErrCompanyUnavailable wrapping the cause", err)
	}
}

func TestCompanyDoesNotCallTheRegisterForAnInvalidNumber(t *testing.T) {
	t.Parallel()

	companies := &fakeCompanies{}
	_, _ = newService(t, &fakeNotices{}, companies).Company(context.Background(), "974760674")
	if len(companies.calls) != 0 {
		t.Errorf("lookups = %v", companies.calls)
	}
}
