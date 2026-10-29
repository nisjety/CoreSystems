package eforms

import (
	"slices"
	"strings"
	"testing"
)

// A buyer in another country has another kind of registry number. The parser reads what the notice
// gives; whether it is a Norwegian organisation number is for the normaliser to say, with a
// recorded problem. Dropping the value at parse time would lose it.
func TestParseKeepsAForeignOrganisationIdentifierAsGiven(t *testing.T) {
	t.Parallel()

	for _, id := range []string{"556000-0000", "FI1234567-8", "123456789"} {
		n, err := Parse(sample("<cbc:CompanyID>000 000 000</cbc:CompanyID>", "<cbc:CompanyID>"+id+"</cbc:CompanyID>"))
		if err != nil {
			t.Fatalf("%q: Parse: %v", id, err)
		}
		if !slices.Equal(n.BuyerOrgNumbers, []string{id}) || len(n.Issues) != 0 {
			t.Errorf("%q: buyer %v issues %v", id, n.BuyerOrgNumbers, n.Issues)
		}
	}
}

func TestParseRemovesSpacesFromAnIdentifierOnly(t *testing.T) {
	t.Parallel()

	n, err := Parse(sample("<cbc:CompanyID>000 000 000</cbc:CompanyID>", "<cbc:CompanyID>  974 760 673  </cbc:CompanyID>"))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if !slices.Equal(n.BuyerOrgNumbers, []string{"974760673"}) {
		t.Errorf("buyer = %v", n.BuyerOrgNumbers)
	}
}

// What is not an identifier at all (markup, control text, an absurd length) is an issue and no
// value, so a hostile notice cannot put free text where a registry number belongs.
func TestParseRefusesWhatIsNotAnIdentifier(t *testing.T) {
	t.Parallel()

	for name, id := range map[string]string{
		"markup":      "&lt;script&gt;alert(1)&lt;/script&gt;",
		"free text":   "ring meg paa mobil",
		"no digit":    "x",
		"too long":    strings.Repeat("9", 65),
		"punctuation": "000;DROP",
	} {
		n, err := Parse(sample("<cbc:CompanyID>000 000 000</cbc:CompanyID>", "<cbc:CompanyID>"+id+"</cbc:CompanyID>"))
		if err != nil {
			t.Fatalf("%s: Parse: %v", name, err)
		}
		if len(n.BuyerOrgNumbers) != 0 {
			t.Errorf("%s: kept %v", name, n.BuyerOrgNumbers)
		}
		if !slices.Contains(n.Issues, "organisation: an identifier does not look like a registry number") {
			t.Errorf("%s: issues = %v", name, n.Issues)
		}
	}
}

func TestParseKeepsAForeignWinnerIdentifier(t *testing.T) {
	t.Parallel()

	n, err := Parse(award("<cbc:CompanyID>111 111 111</cbc:CompanyID>", "<cbc:CompanyID>556000-0000</cbc:CompanyID>"))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if got := n.Results[0].Winners; len(got) != 1 || got[0].OrgNumber != "556000-0000" {
		t.Errorf("winners = %+v, want the identifier as given", got)
	}
}
