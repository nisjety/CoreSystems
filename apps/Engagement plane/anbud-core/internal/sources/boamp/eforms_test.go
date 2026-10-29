package boamp

import (
	"encoding/json"
	"encoding/xml"
	"errors"
	"os"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/eforms"
)

// fixture is three records of one captured day (testdata/records_2026-10-29.json): a contract notice
// (26-104530), a contract award (26-104549) and a French national form (26-105149).
type fixture struct {
	Results []record `json:"results"`
}

func loadFixture(t *testing.T) map[string]record {
	t.Helper()
	raw, err := os.ReadFile("testdata/records_2026-10-29.json")
	if err != nil {
		t.Fatal(err)
	}
	var f fixture
	if err := json.Unmarshal(raw, &f); err != nil {
		t.Fatal(err)
	}
	byID := map[string]record{}
	for _, r := range f.Results {
		byID[r.Idweb] = r
	}
	return byID
}

// TestConvertedContractNoticeReadsAsItsNotice converts a BOAMP contract notice and parses the XML with
// the eForms parser: the identifiers and the type are the notice's own, as the source JSON states them.
func TestConvertedContractNoticeReadsAsItsNotice(t *testing.T) {
	t.Parallel()
	r := loadFixture(t)["26-104530"]
	body, err := eFormsXML([]byte(r.Donnees))
	if err != nil {
		t.Fatalf("eFormsXML: %v", err)
	}
	n, err := eforms.Parse(body)
	if err != nil {
		t.Fatalf("eforms.Parse: %v", err)
	}
	if n.Root != "ContractNotice" || n.NoticeTypeCode != "cn-standard" || n.SDKVersion != "1.13" {
		t.Errorf("header = root %q, type %q, sdk %q; want ContractNotice, cn-standard, 1.13", n.Root, n.NoticeTypeCode, n.SDKVersion)
	}
	if n.NoticeID != "8def4022-f6c6-4004-ae52-fc3e6a141766" {
		t.Errorf("NoticeID = %q, want the notice's cbc:ID", n.NoticeID)
	}
	if n.ProcedureID != "3418fccb-a455-41bf-932e-4bad168da319" {
		t.Errorf("ProcedureID = %q, want the contract folder id", n.ProcedureID)
	}
	if !strings.HasPrefix(n.Title, "Appel à candidatures") {
		t.Errorf("Title = %q, want the procurement's title in French", n.Title)
	}
}

// TestConvertedContractAwardReadsAsAward converts a BOAMP contract award and parses it: the award path
// of the parser reads it, which is what the sync needs from an award.
func TestConvertedContractAwardReadsAsAward(t *testing.T) {
	t.Parallel()
	r := loadFixture(t)["26-104549"]
	body, err := eFormsXML([]byte(r.Donnees))
	if err != nil {
		t.Fatalf("eFormsXML: %v", err)
	}
	n, err := eforms.Parse(body)
	if err != nil {
		t.Fatalf("eforms.Parse: %v", err)
	}
	if n.Root != "ContractAwardNotice" || n.NoticeTypeCode == "" {
		t.Errorf("header = root %q, type %q; want a ContractAwardNotice with a notice type", n.Root, n.NoticeTypeCode)
	}
}

// TestConversionIsDeterministic: the body's hash is the evidence's identity, so one record converts to
// the same bytes every time.
func TestConversionIsDeterministic(t *testing.T) {
	t.Parallel()
	r := loadFixture(t)["26-104530"]
	first, err := eFormsXML([]byte(r.Donnees))
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 3; i++ {
		again, err := eFormsXML([]byte(r.Donnees))
		if err != nil {
			t.Fatal(err)
		}
		if string(again) != string(first) {
			t.Fatal("the same record converted to different bytes")
		}
	}
}

// TestNationalFormKeepsItsJSON: a French national form is not eForms. It is kept as its own JSON, and
// the eForms parser refuses it.
func TestNationalFormKeepsItsJSON(t *testing.T) {
	t.Parallel()
	r := loadFixture(t)["26-105149"]
	if _, err := eFormsXML([]byte(r.Donnees)); !errors.Is(err, errNotEForms) {
		t.Fatalf("eFormsXML err = %v, want errNotEForms", err)
	}
	body, contentType := noticeBody(r.Donnees)
	if contentType != "application/json" || string(body) != r.Donnees {
		t.Fatalf("noticeBody = %q, %q; want the record's JSON as application/json", contentType, body)
	}
	if _, err := eforms.Parse(body); err == nil {
		t.Fatal("the eForms parser read a national form")
	}
}

// TestConversionRefusesAPrefixItDoesNotKnow: an element in a namespace the eForms parser does not read
// is refused, and the record keeps its JSON rather than a guessed namespace.
func TestConversionRefusesAPrefixItDoesNotKnow(t *testing.T) {
	t.Parallel()
	donnees := `{"EFORMS":{"ContractNotice":{"cbc:ID":"x","sdk:Other":"y"}}}`
	if _, err := eFormsXML([]byte(donnees)); err == nil || errors.Is(err, errNotEForms) {
		t.Fatalf("eFormsXML err = %v, want a refusal for the unknown prefix", err)
	}
	if _, contentType := noticeBody(donnees); contentType != "application/json" {
		t.Fatalf("content type = %q, want application/json", contentType)
	}
}

// TestConversionKeepsDocumentOrder: members are written in the order the JSON gives them. The order
// is part of the notice's bytes, so a Go map (which has none) would make the hash unstable.
func TestConversionKeepsDocumentOrder(t *testing.T) {
	t.Parallel()
	donnees := `{"EFORMS":{"ContractNotice":{"cbc:Zeta":"z","cbc:Alpha":"a"}}}`
	body, err := eFormsXML([]byte(donnees))
	if err != nil {
		t.Fatal(err)
	}
	s := string(body)
	if strings.Index(s, "Zeta") > strings.Index(s, "Alpha") {
		t.Fatalf("Zeta was written after Alpha: %s", s)
	}
}

// TestConversionEscapesText: text and attribute values are escaped, so a value with markup characters
// stays text and the XML still reads.
func TestConversionEscapesText(t *testing.T) {
	t.Parallel()
	donnees := `{"EFORMS":{"ContractNotice":{"cbc:ID":{"@schemeName":"a\"b<c","#text":"x < y & z > \"q\""}}}}`
	body, err := eFormsXML([]byte(donnees))
	if err != nil {
		t.Fatal(err)
	}
	var doc struct {
		ID struct {
			Scheme string `xml:"schemeName,attr"`
			Text   string `xml:",chardata"`
		} `xml:"ID"`
	}
	if err := xml.Unmarshal(body, &doc); err != nil {
		t.Fatalf("the converted XML does not read: %v\n%s", err, body)
	}
	if doc.ID.Scheme != `a"b<c` || doc.ID.Text != `x < y & z > "q"` {
		t.Fatalf("round trip = %q, %q", doc.ID.Scheme, doc.ID.Text)
	}
}

// TestConversionRefusesAnUnnamespacedChild: a child element without a prefix would take the root's
// default namespace, which is the wrong one; it is refused.
func TestConversionRefusesAnUnnamespacedChild(t *testing.T) {
	t.Parallel()
	donnees := `{"EFORMS":{"ContractNotice":{"Loose":"x"}}}`
	if _, err := eFormsXML([]byte(donnees)); err == nil {
		t.Fatal("an unprefixed child was converted")
	}
}
