package eforms

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"
)

// sampleNotice is a synthetic ContractNotice in the shape of eForms SDK 1.14, flavour eu, laid
// out as the captured notices are: the buyer's details sit in the organisation list of the
// extension block and the party refers to them; document links and qualification requirements
// sit per lot. Every name, number and address is invented, and the e-mail domain is reserved.
const sampleNotice = `<?xml version="1.0" encoding="UTF-8"?>
<ContractNotice xmlns="urn:oasis:names:specification:ubl:schema:xsd:ContractNotice-2"
 xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2"
 xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
 xmlns:ext="urn:oasis:names:specification:ubl:schema:xsd:CommonExtensionComponents-2"
 xmlns:efac="http://data.europa.eu/p27/eforms-ubl-extension-aggregate-components/1"
 xmlns:efext="http://data.europa.eu/p27/eforms-ubl-extensions/1">
  <ext:UBLExtensions><ext:UBLExtension><ext:ExtensionContent><efext:EformsExtension>
    <efac:NoticeSubType><cbc:SubTypeCode listName="notice-subtype">16</cbc:SubTypeCode></efac:NoticeSubType>
    <efac:Organizations>
      <efac:Organization>
        <efac:Company>
          <cac:PartyIdentification><cbc:ID>ORG-0001</cbc:ID></cac:PartyIdentification>
          <cac:PartyLegalEntity><cbc:CompanyID>000 000 000</cbc:CompanyID></cac:PartyLegalEntity>
          <cac:PartyName><cbc:Name>Eksempelkommune</cbc:Name></cac:PartyName>
          <cac:Contact><cbc:Name>Fiktiv Person</cbc:Name><cbc:ElectronicMail>person@example.invalid</cbc:ElectronicMail></cac:Contact>
        </efac:Company>
      </efac:Organization>
    </efac:Organizations>
  </efext:EformsExtension></ext:ExtensionContent></ext:UBLExtension></ext:UBLExtensions>
  <cbc:UBLVersionID>2.3</cbc:UBLVersionID>
  <cbc:CustomizationID>eforms-sdk-1.14#extended#urn:fdc:anskaffelser.no:2023:eforms:eu</cbc:CustomizationID>
  <cbc:ID>00000000-0000-4000-8000-0000000000bb</cbc:ID>
  <cbc:VersionID>01</cbc:VersionID>
  <cbc:ContractFolderID>00000000-0000-4000-8000-0000000000aa</cbc:ContractFolderID>
  <cbc:NoticeTypeCode>cn-standard</cbc:NoticeTypeCode>
  <cac:ProcurementProject>
    <cbc:Name>Eksempeltjeneste</cbc:Name>
    <cbc:Description>Fiktiv beskrivelse av en tjeneste.</cbc:Description>
    <cbc:ProcurementTypeCode>services</cbc:ProcurementTypeCode>
    <cac:RequestedTenderTotal><cbc:EstimatedOverallContractAmount currencyID="NOK">1500000.00</cbc:EstimatedOverallContractAmount></cac:RequestedTenderTotal>
    <cac:MainCommodityClassification><cbc:ItemClassificationCode listName="cpv">72000000</cbc:ItemClassificationCode></cac:MainCommodityClassification>
    <cac:AdditionalCommodityClassification><cbc:ItemClassificationCode listName="cpv">72212000</cbc:ItemClassificationCode></cac:AdditionalCommodityClassification>
    <cac:RealizedLocation><cac:Address><cbc:CountrySubentityCode>NO081</cbc:CountrySubentityCode></cac:Address></cac:RealizedLocation>
  </cac:ProcurementProject>
  <cac:TenderingTerms>
    <cac:TendererQualificationRequest><cac:SpecificTendererRequirement><cbc:TendererRequirementTypeCode>CERTIFICATE</cbc:TendererRequirementTypeCode><cbc:Description>Fiktivt sertifikatkrav</cbc:Description></cac:SpecificTendererRequirement></cac:TendererQualificationRequest>
  </cac:TenderingTerms>
  <cac:TenderingProcess>
    <cac:TenderSubmissionDeadlinePeriod><cbc:EndDate>2026-11-15</cbc:EndDate><cbc:EndTime>12:00:00+01:00</cbc:EndTime></cac:TenderSubmissionDeadlinePeriod>
  </cac:TenderingProcess>
  <cac:ContractingParty>
    <cac:Party><cac:PartyIdentification><cbc:ID>ORG-0001</cbc:ID></cac:PartyIdentification></cac:Party>
  </cac:ContractingParty>
  <cac:ProcurementProjectLot>
    <cbc:ID schemeName="Lot">LOT-0001</cbc:ID>
    <cac:ProcurementProject>
      <cbc:Name>Delkontrakt 1</cbc:Name>
      <cac:RequestedTenderTotal><cbc:EstimatedOverallContractAmount currencyID="NOK">500000</cbc:EstimatedOverallContractAmount></cac:RequestedTenderTotal>
      <cac:MainCommodityClassification><cbc:ItemClassificationCode listName="cpv">72212000</cbc:ItemClassificationCode></cac:MainCommodityClassification>
    </cac:ProcurementProject>
    <cac:TenderingTerms>
      <cac:CallForTendersDocumentReference><cac:Attachment><cac:ExternalReference><cbc:URI>https://example.invalid/docs/1</cbc:URI></cac:ExternalReference></cac:Attachment></cac:CallForTendersDocumentReference>
      <cac:TendererQualificationRequest><cac:SpecificTendererRequirement><cbc:TendererRequirementTypeCode>TURNOVER</cbc:TendererRequirementTypeCode><cbc:Description>Fiktivt omsetningskrav</cbc:Description></cac:SpecificTendererRequirement></cac:TendererQualificationRequest>
    </cac:TenderingTerms>
    <cac:TenderingProcess><cac:TenderSubmissionDeadlinePeriod><cbc:EndDate>2026-11-20</cbc:EndDate><cbc:EndTime>23:59:00Z</cbc:EndTime></cac:TenderSubmissionDeadlinePeriod></cac:TenderingProcess>
  </cac:ProcurementProjectLot>
</ContractNotice>`

// sample returns the synthetic notice with the given find/replace pairs applied.
func sample(pairs ...string) []byte {
	return []byte(strings.NewReplacer(pairs...).Replace(sampleNotice))
}

func TestParseReadsTheCoreFieldsOfAContractNotice(t *testing.T) {
	t.Parallel()

	n, err := Parse(sample())
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if n.Root != "ContractNotice" || n.SDKVersion != "1.14" || n.Flavour != FlavourEU || n.NoticeID != "00000000-0000-4000-8000-0000000000bb" {
		t.Errorf("header = root %q sdk %q flavour %q id %q", n.Root, n.SDKVersion, n.Flavour, n.NoticeID)
	}
	if n.Title != "Eksempeltjeneste" || n.ProcurementType != "services" {
		t.Errorf("title and procurement type = %q, %q", n.Title, n.ProcurementType)
	}
	if n.EstimatedValue == nil || *n.EstimatedValue != (Amount{Minor: 150000000, Currency: "NOK"}) {
		t.Errorf("estimated value = %+v, want 1 500 000,00 NOK in øre", n.EstimatedValue)
	}
	if !slices.Equal(n.CPVMain, []string{"72000000"}) || !slices.Equal(n.CPVAdditional, []string{"72212000"}) {
		t.Errorf("CPV main %v, additional %v", n.CPVMain, n.CPVAdditional)
	}
	if !slices.Equal(n.NUTS, []string{"NO081"}) {
		t.Errorf("NUTS = %v, want [NO081]", n.NUTS)
	}
	want := time.Date(2026, time.November, 15, 11, 0, 0, 0, time.UTC)
	if n.Deadline == nil || !n.Deadline.Equal(want) {
		t.Errorf("deadline = %v, want %v (12:00 at +01:00)", n.Deadline, want)
	}
	if len(n.Lots) != 1 {
		t.Fatalf("lots = %d, want 1", len(n.Lots))
	}
	lot := n.Lots[0]
	if lot.ID != "LOT-0001" || lot.Title != "Delkontrakt 1" || !slices.Equal(lot.CPVMain, []string{"72212000"}) {
		t.Errorf("lot = %+v", lot)
	}
	if lot.Value == nil || *lot.Value != (Amount{Minor: 50000000, Currency: "NOK"}) {
		t.Errorf("lot value = %+v, want 500 000,00 NOK", lot.Value)
	}
	lotWant := time.Date(2026, time.November, 20, 23, 59, 0, 0, time.UTC)
	if lot.Deadline == nil || !lot.Deadline.Equal(lotWant) {
		t.Errorf("lot deadline = %v, want %v", lot.Deadline, lotWant)
	}
	if len(n.Issues) != 0 {
		t.Errorf("issues = %v, want none for a complete notice", n.Issues)
	}
}

// The buyer is the organisation the party refers to in the organisation list: its name and its
// organisation number, with the spaces removed, are read from there.
func TestParseResolvesTheBuyerThroughTheOrganisationList(t *testing.T) {
	t.Parallel()

	n, err := Parse(sample())
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if !slices.Equal(n.BuyerNames, []string{"Eksempelkommune"}) || !slices.Equal(n.BuyerOrgNumbers, []string{"000000000"}) {
		t.Errorf("buyer names %v, org numbers %v; the number must lose its spaces", n.BuyerNames, n.BuyerOrgNumbers)
	}
	if n.ContactPoints != 1 {
		t.Errorf("contact points = %d, want the one contact of the organisation list counted", n.ContactPoints)
	}
}

// Contact persons are counted and never copied: a notice's names, e-mail addresses and
// telephone numbers must not reach the parsed model, so that nothing downstream can show or
// store them by accident.
func TestParseNeverKeepsContactPersons(t *testing.T) {
	t.Parallel()

	n, err := Parse(sample())
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	out, err := json.Marshal(n)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	for _, personal := range []string{"Fiktiv Person", "person@example.invalid"} {
		if strings.Contains(string(out), personal) {
			t.Errorf("the parsed notice carries contact data of the notice")
		}
	}
}

// Document links are read per lot, with the lot they belong to. A link that is not http or https
// is an issue, not a link.
func TestParseReadsDocumentLinksPerLot(t *testing.T) {
	t.Parallel()

	n, err := Parse(sample())
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if len(n.Documents) != 1 || n.Documents[0].URI != "https://example.invalid/docs/1" ||
		n.Documents[0].Lot != "LOT-0001" || n.Documents[0].Access != AccessUnknown {
		t.Errorf("documents = %+v, want one link of lot LOT-0001 with the access flag unknown", n.Documents)
	}
	n, err = Parse(sample("https://example.invalid/docs/1", "ftp://example.invalid/docs/1"))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if len(n.Documents) != 0 || len(n.Issues) == 0 {
		t.Errorf("a link that is not http or https: documents %v, issues %v", n.Documents, n.Issues)
	}
}

// Qualification requirements are read at notice level and per lot, with the type given and the
// text as written. Whether a requirement is mandatory is not read yet: no field of this shape
// says so, and the parser does not guess.
func TestParseReadsQualificationRequirements(t *testing.T) {
	t.Parallel()

	n, err := Parse(sample())
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	want := []Requirement{
		{Lot: "", Type: "CERTIFICATE", Text: "Fiktivt sertifikatkrav"},
		{Lot: "LOT-0001", Type: "TURNOVER", Text: "Fiktivt omsetningskrav"},
	}
	if !slices.Equal(n.Requirements, want) {
		t.Errorf("requirements = %+v, want %+v", n.Requirements, want)
	}
}

// A date may carry its zone, as the captured notices do: the calendar day is the day written, and
// the time, with its own zone, gives the hour. The instant is the same as without the zone.
func TestParseAcceptsADateWithItsZone(t *testing.T) {
	t.Parallel()

	n, err := Parse(sample("<cbc:EndDate>2026-11-15</cbc:EndDate>", "<cbc:EndDate>2026-11-15+01:00</cbc:EndDate>"))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	want := time.Date(2026, time.November, 15, 11, 0, 0, 0, time.UTC)
	if n.Deadline == nil || !n.Deadline.Equal(want) || len(n.Issues) != 0 {
		t.Errorf("deadline = %v, issues %v; want %v and no issue", n.Deadline, n.Issues, want)
	}
}

// A time with fractional seconds is read as the same hour.
func TestParseAcceptsFractionalSeconds(t *testing.T) {
	t.Parallel()

	n, err := Parse(sample("<cbc:EndTime>12:00:00+01:00</cbc:EndTime>", "<cbc:EndTime>12:00:00.000+01:00</cbc:EndTime>"))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	want := time.Date(2026, time.November, 15, 11, 0, 0, 0, time.UTC)
	if n.Deadline == nil || !n.Deadline.Equal(want) {
		t.Errorf("deadline = %v, want %v", n.Deadline, want)
	}
}

// A notice in a shape with no field map is refused, never parsed by guesswork: a different SDK
// version, another country's extension, another root element. A plain EU notice is no longer among
// them (2026-10-28): TestPlainTEDNoticesReadLikeTheirNorwegianFlavour.
func TestParseRefusesAShapeWithNoFieldMap(t *testing.T) {
	t.Parallel()

	cases := map[string][]byte{
		"an SDK version with no field map": sample("eforms-sdk-1.14#", "eforms-sdk-1.8#"),
		"another country's extension":      sample("urn:fdc:anskaffelser.no:2023:eforms:eu", "urn:fdc:example.eu:2024:eforms:eu"),
		"a prior information root of an unknown flavour": sample("<ContractNotice ", "<PriorInformationNotice ", "</ContractNotice>", "</PriorInformationNotice>",
			"xsd:ContractNotice-2", "xsd:PriorInformationNotice-2", "eforms:eu", "eforms:future"),
		"a contract award notice of an unknown flavour": sample("<ContractNotice ", "<ContractAwardNotice ", "</ContractNotice>", "</ContractAwardNotice>",
			"xsd:ContractNotice-2", "xsd:ContractAwardNotice-2", "eforms:eu", "eforms:future"),
	}
	for name, doc := range cases {
		if _, err := Parse(doc); !errors.Is(err, ErrNoFieldMap) {
			t.Errorf("%s: err = %v, want ErrNoFieldMap", name, err)
		}
	}
}

func TestParseRefusesWhatIsNotEForms(t *testing.T) {
	t.Parallel()

	if _, err := Parse([]byte("<html><body>not a notice</body></html>")); !errors.Is(err, ErrNotEForms) {
		t.Errorf("err = %v, want ErrNotEForms", err)
	}
}

// What the field maps do not name is reported by element path, so a notice that holds more than
// the parser reads is visible to whoever maintains it. Names only, never values, and the
// extension blocks that were read are not a finding.
func TestParseReportsWhatItDidNotMap(t *testing.T) {
	t.Parallel()

	n, err := Parse(sample("<cbc:Name>Eksempeltjeneste</cbc:Name>", "<cbc:Name>Eksempeltjeneste</cbc:Name><cbc:Unknown>x</cbc:Unknown>"))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if !slices.Contains(n.Unmapped, "ContractNotice/ProcurementProject/Unknown") {
		t.Errorf("unmapped = %v, want the unknown element reported by path", n.Unmapped)
	}
	if len(n.Unmapped) != 1 {
		t.Errorf("unmapped = %v, want only the one unknown element of the sample", n.Unmapped)
	}
	for _, path := range n.Unmapped {
		if strings.Contains(path, "Fiktiv") || strings.Contains(path, "example.invalid") {
			t.Errorf("an unmapped path carries a value")
		}
	}
}

// A value that is not a plain decimal is not guessed at: the field is left empty and an issue
// records that it was there, with fixed text and no value.
func TestParseDoesNotGuessAnAmount(t *testing.T) {
	t.Parallel()

	n, err := Parse(sample("1500000.00", "1 500 000,00"))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if n.EstimatedValue != nil {
		t.Errorf("estimated value = %+v, want none for a value that is not a decimal", n.EstimatedValue)
	}
	if len(n.Issues) == 0 {
		t.Error("no issue recorded for a value that is not a decimal")
	}
	for _, issue := range n.Issues {
		if strings.Contains(issue, "500") {
			t.Errorf("an issue quotes the value: %q", issue)
		}
	}
}

// A deadline with no time, or with no zone, is not a deadline anyone can act on at the hour: it
// is left empty with an issue, rather than read in some zone the parser made up.
func TestParseDoesNotInventADeadlineTime(t *testing.T) {
	t.Parallel()

	noTime, err := Parse(sample("<cbc:EndTime>12:00:00+01:00</cbc:EndTime>", ""))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if noTime.Deadline != nil || len(noTime.Issues) == 0 {
		t.Errorf("deadline without a time: deadline %v, issues %v", noTime.Deadline, noTime.Issues)
	}
	noZone, err := Parse(sample("<cbc:EndTime>12:00:00+01:00</cbc:EndTime>", "<cbc:EndTime>12:00:00</cbc:EndTime>"))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if noZone.Deadline != nil || len(noZone.Issues) == 0 {
		t.Errorf("deadline without a zone: deadline %v, issues %v", noZone.Deadline, noZone.Issues)
	}
}

// Every captured notice that has a field map parses, and every other one is refused by name. The
// test prints counts and fixed text only: never a value, an identifier or a file's contents.
func TestParseCapturedNoticesCountsOnly(t *testing.T) {
	files, err := filepath.Glob("../../testdata/captured/*.xml")
	if err != nil {
		t.Fatal(err)
	}
	if len(files) == 0 {
		t.Skip("no captured notices; run: go run ./cmd/doffin-probe -discover=false -samples 12 -save-dir testdata/captured")
	}

	var parsed, noFieldMap, other int
	shapes := map[string]int{}        // root, SDK, flavour: how many notices of each shape
	present := map[string]int{}       // field: how many parsed notices state it
	issueKinds := map[string]int{}    // fixed issue text: how many times it occurred
	unmappedPaths := map[string]int{} // element path: how many notices hold it unread
	for _, file := range files {
		name := filepath.Base(file)
		data, err := os.ReadFile(file)
		if err != nil {
			t.Errorf("%s: %v", name, err)
			continue
		}
		h, herr := Sniff(data)
		if herr == nil {
			shapes[h.Root+" sdk "+h.SDKVersion+" flavour "+string(h.Flavour)]++
		}
		n, err := Parse(data)
		switch {
		case err == nil:
			parsed++
			if n.NoticeID == "" || (len(n.CPVMain) == 0 && len(n.Issues) == 0) {
				t.Errorf("%s: parsed without an identifier, or without a CPV code or an issue to say why", name)
			}
			for field, ok := range map[string]bool{
				"title": n.Title != "", "cpv": len(n.CPVMain) > 0, "nuts": len(n.NUTS) > 0,
				"estimated value": n.EstimatedValue != nil, "deadline": n.Deadline != nil,
				"buyer name": len(n.BuyerNames) > 0, "buyer org number": len(n.BuyerOrgNumbers) > 0,
				"contact point": n.ContactPoints > 0, "document link": len(n.Documents) > 0,
				"requirement": len(n.Requirements) > 0, "lot": len(n.Lots) > 0,
				"lot deadline": lotsWithDeadline(n.Lots) > 0, "lot value": lotsWithValue(n.Lots) > 0,
				"lot period":      lotsWith(n.Lots, func(l Lot) bool { return l.Period != nil }) > 0,
				"lot renewals":    lotsWith(n.Lots, func(l Lot) bool { return l.Renewals != nil }) > 0,
				"contract signed": resultsSigned(n.Results) > 0,
				"issued at":       n.IssuedAt != nil, "change section": n.Change != nil,
				"participation deadline": participation(n) > 0, "selection criterion": len(n.SelectionCriteria) > 0,
				"procedure type": n.ProcedureType != "", "award criterion": criteria(n.Lots, false) > 0,
				"weighted criterion": criteria(n.Lots, true) > 0, "result": len(n.Results) > 0,
				"winner": winners(n.Results, false) > 0, "winner org number": winners(n.Results, true) > 0,
				"tender count": tenderCounts(n.Results) > 0, "awarded value": n.AwardedValue != nil,
				"award date": n.AwardDate != nil,
			} {
				if ok {
					present[field]++
				}
			}
			for _, issue := range n.Issues {
				issueKinds[issue]++
			}
			for _, path := range n.Unmapped {
				unmappedPaths[path]++
			}
		case errors.Is(err, ErrNoFieldMap):
			noFieldMap++
		default:
			other++
			t.Errorf("%s: %v", name, err)
		}
	}
	t.Logf("%d captured notices: parsed %d, no field map %d, other errors %d", len(files), parsed, noFieldMap, other)
	t.Logf("shapes: %v", shapes)
	t.Logf("fields stated by the %d parsed notices: %v", parsed, present)
	t.Logf("issues: %v", issueKinds)
	t.Logf("unmapped element paths: %v", unmappedPaths)
}

func lotsWithDeadline(lots []Lot) int {
	count := 0
	for _, l := range lots {
		if l.Deadline != nil {
			count++
		}
	}
	return count
}

func lotsWithValue(lots []Lot) int {
	count := 0
	for _, l := range lots {
		if l.Value != nil {
			count++
		}
	}
	return count
}

func criteria(lots []Lot, weighted bool) int {
	count := 0
	for _, l := range lots {
		for _, c := range l.AwardCriteria {
			if !weighted || c.Weight != nil {
				count++
			}
		}
	}
	return count
}

func winners(results []LotResult, withOrgNumber bool) int {
	count := 0
	for _, r := range results {
		for _, w := range r.Winners {
			if !withOrgNumber || w.OrgNumber != "" {
				count++
			}
		}
	}
	return count
}

func tenderCounts(results []LotResult) int {
	count := 0
	for _, r := range results {
		if r.TendersReceived != nil {
			count++
		}
	}
	return count
}

// participation counts the deadlines for requests to participate the notice states, at notice level
// and in its lots.
func participation(n Notice) int {
	count := 0
	if n.ParticipationDeadline != nil {
		count++
	}
	for _, l := range n.Lots {
		if l.ParticipationDeadline != nil {
			count++
		}
	}
	return count
}

func lotsWith(lots []Lot, ok func(Lot) bool) int {
	count := 0
	for _, l := range lots {
		if ok(l) {
			count++
		}
	}
	return count
}

func resultsSigned(results []LotResult) int {
	count := 0
	for _, r := range results {
		if r.ContractSigned != nil {
			count++
		}
	}
	return count
}
