package eforms

import (
	"encoding/json"
	"errors"
	"slices"
	"strings"
	"testing"
	"time"
)

// sampleAward is a synthetic ContractAwardNotice in the shape of eForms SDK 1.14, flavour eu, laid
// out as the captured award notices are: the result sits in the extension block, a lot result
// points at a tender, the tender at a tendering party, and the party at an organisation of the
// organisation list. Every name, number and address is invented, and the e-mail domain is reserved.
const sampleAward = `<?xml version="1.0" encoding="UTF-8"?>
<ContractAwardNotice xmlns="urn:oasis:names:specification:ubl:schema:xsd:ContractAwardNotice-2"
 xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2"
 xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
 xmlns:ext="urn:oasis:names:specification:ubl:schema:xsd:CommonExtensionComponents-2"
 xmlns:efac="http://data.europa.eu/p27/eforms-ubl-extension-aggregate-components/1"
 xmlns:efbc="http://data.europa.eu/p27/eforms-ubl-extension-basic-components/1"
 xmlns:efext="http://data.europa.eu/p27/eforms-ubl-extensions/1">
  <ext:UBLExtensions><ext:UBLExtension><ext:ExtensionContent><efext:EformsExtension>
    <efac:NoticeResult>
      <cbc:TotalAmount currencyID="NOK">2400000</cbc:TotalAmount>
      <efac:LotResult>
        <cbc:ID schemeName="result">RES-0001</cbc:ID>
        <cbc:TenderResultCode listName="winner-selection-status">selec-w</cbc:TenderResultCode>
        <efac:LotTender><cbc:ID schemeName="tender">TEN-0001</cbc:ID></efac:LotTender>
        <efac:ReceivedSubmissionsStatistics><efbc:StatisticsCode listName="received-submission-type">t-sme</efbc:StatisticsCode><efbc:StatisticsNumeric>5</efbc:StatisticsNumeric></efac:ReceivedSubmissionsStatistics>
        <efac:ReceivedSubmissionsStatistics><efbc:StatisticsCode listName="received-submission-type">tenders</efbc:StatisticsCode><efbc:StatisticsNumeric>7</efbc:StatisticsNumeric></efac:ReceivedSubmissionsStatistics>
        <efac:SettledContract><cbc:ID schemeName="contract">CON-0001</cbc:ID></efac:SettledContract>
        <efac:TenderLot><cbc:ID schemeName="Lot">LOT-0001</cbc:ID></efac:TenderLot>
      </efac:LotResult>
      <efac:LotResult>
        <cbc:ID schemeName="result">RES-0002</cbc:ID>
        <cbc:TenderResultCode listName="winner-selection-status">clos-nw</cbc:TenderResultCode>
        <efac:TenderLot><cbc:ID schemeName="Lot">LOT-0002</cbc:ID></efac:TenderLot>
      </efac:LotResult>
      <efac:LotTender>
        <cbc:ID schemeName="tender">TEN-0001</cbc:ID>
        <cac:LegalMonetaryTotal><cbc:PayableAmount currencyID="NOK">2400000</cbc:PayableAmount></cac:LegalMonetaryTotal>
        <efac:TenderingParty><cbc:ID schemeName="tendering-party">TPA-0001</cbc:ID></efac:TenderingParty>
        <efac:TenderLot><cbc:ID schemeName="Lot">LOT-0001</cbc:ID></efac:TenderLot>
      </efac:LotTender>
      <efac:SettledContract>
        <cbc:ID schemeName="contract">CON-0001</cbc:ID>
        <cbc:IssueDate>2026-09-01+02:00</cbc:IssueDate>
        <efac:LotTender><cbc:ID schemeName="tender">TEN-0001</cbc:ID></efac:LotTender>
      </efac:SettledContract>
      <efac:TenderingParty>
        <cbc:ID schemeName="tendering-party">TPA-0001</cbc:ID>
        <efac:Tenderer><cbc:ID schemeName="organization">ORG-0002</cbc:ID></efac:Tenderer>
      </efac:TenderingParty>
    </efac:NoticeResult>
    <efac:NoticeSubType><cbc:SubTypeCode listName="notice-subtype">29</cbc:SubTypeCode></efac:NoticeSubType>
    <efac:Organizations>
      <efac:Organization>
        <efac:Company>
          <cac:PartyIdentification><cbc:ID>ORG-0001</cbc:ID></cac:PartyIdentification>
          <cac:PartyLegalEntity><cbc:CompanyID>000000000</cbc:CompanyID></cac:PartyLegalEntity>
          <cac:PartyName><cbc:Name>Eksempelkommune</cbc:Name></cac:PartyName>
          <cac:Contact><cbc:Name>Fiktiv Person</cbc:Name><cbc:ElectronicMail>person@example.invalid</cbc:ElectronicMail></cac:Contact>
        </efac:Company>
      </efac:Organization>
      <efac:Organization>
        <efac:Company>
          <cac:PartyIdentification><cbc:ID>ORG-0002</cbc:ID></cac:PartyIdentification>
          <cac:PartyLegalEntity><cbc:CompanyID>111 111 111</cbc:CompanyID></cac:PartyLegalEntity>
          <cac:PartyName><cbc:Name>Fiktiv Leverandør AS</cbc:Name></cac:PartyName>
          <cac:Contact><cbc:Name>Annen Fiktiv</cbc:Name><cbc:Telephone>+47 00000000</cbc:Telephone></cac:Contact>
        </efac:Company>
      </efac:Organization>
    </efac:Organizations>
  </efext:EformsExtension></ext:ExtensionContent></ext:UBLExtension></ext:UBLExtensions>
  <cbc:UBLVersionID>2.3</cbc:UBLVersionID>
  <cbc:CustomizationID>eforms-sdk-1.14#extended#urn:fdc:anskaffelser.no:2023:eforms:eu</cbc:CustomizationID>
  <cbc:ID>00000000-0000-4000-8000-0000000000cc</cbc:ID>
  <cbc:VersionID>01</cbc:VersionID>
  <cbc:ContractFolderID>00000000-0000-4000-8000-0000000000aa</cbc:ContractFolderID>
  <cbc:NoticeTypeCode listName="result">can-standard</cbc:NoticeTypeCode>
  <cac:ContractingParty>
    <cac:Party><cac:PartyIdentification><cbc:ID>ORG-0001</cbc:ID></cac:PartyIdentification></cac:Party>
  </cac:ContractingParty>
  <cac:TenderingProcess><cbc:ProcedureCode listName="procurement-procedure-type">open</cbc:ProcedureCode></cac:TenderingProcess>
  <cac:ProcurementProject>
    <cbc:Name>Eksempeltjeneste</cbc:Name>
    <cbc:ProcurementTypeCode listName="contract-nature">services</cbc:ProcurementTypeCode>
    <cac:MainCommodityClassification><cbc:ItemClassificationCode listName="cpv">72000000</cbc:ItemClassificationCode></cac:MainCommodityClassification>
    <cac:RealizedLocation><cac:Address><cbc:CountrySubentityCode listName="nuts">NO081</cbc:CountrySubentityCode></cac:Address></cac:RealizedLocation>
  </cac:ProcurementProject>
  <cac:ProcurementProjectLot>
    <cbc:ID schemeName="Lot">LOT-0001</cbc:ID>
    <cac:TenderingTerms>
      <cac:AwardingTerms><cac:AwardingCriterion>
        <cac:SubordinateAwardingCriterion>
          <ext:UBLExtensions><ext:UBLExtension><ext:ExtensionContent><efext:EformsExtension>
            <efac:AwardCriterionParameter><efbc:ParameterCode listName="number-weight">per-exa</efbc:ParameterCode><efbc:ParameterNumeric>60</efbc:ParameterNumeric></efac:AwardCriterionParameter>
          </efext:EformsExtension></ext:ExtensionContent></ext:UBLExtension></ext:UBLExtensions>
          <cbc:AwardingCriterionTypeCode listName="award-criterion-type">quality</cbc:AwardingCriterionTypeCode>
          <cbc:Name>Kvalitet</cbc:Name>
          <cbc:Description>Fiktiv beskrivelse av kvalitetskriteriet.</cbc:Description>
        </cac:SubordinateAwardingCriterion>
        <cac:SubordinateAwardingCriterion>
          <ext:UBLExtensions><ext:UBLExtension><ext:ExtensionContent><efext:EformsExtension>
            <efac:AwardCriterionParameter><efbc:ParameterCode listName="number-weight">per-exa</efbc:ParameterCode><efbc:ParameterNumeric>40</efbc:ParameterNumeric></efac:AwardCriterionParameter>
          </efext:EformsExtension></ext:ExtensionContent></ext:UBLExtension></ext:UBLExtensions>
          <cbc:AwardingCriterionTypeCode listName="award-criterion-type">price</cbc:AwardingCriterionTypeCode>
          <cbc:Description>Pris.</cbc:Description>
        </cac:SubordinateAwardingCriterion>
      </cac:AwardingCriterion></cac:AwardingTerms>
    </cac:TenderingTerms>
    <cac:ProcurementProject>
      <cbc:Name>Delkontrakt 1</cbc:Name>
      <cac:MainCommodityClassification><cbc:ItemClassificationCode listName="cpv">72212000</cbc:ItemClassificationCode></cac:MainCommodityClassification>
    </cac:ProcurementProject>
  </cac:ProcurementProjectLot>
  <cac:ProcurementProjectLot>
    <cbc:ID schemeName="Lot">LOT-0002</cbc:ID>
    <cac:ProcurementProject><cbc:Name>Delkontrakt 2</cbc:Name></cac:ProcurementProject>
  </cac:ProcurementProjectLot>
  <cac:TenderResult><cbc:AwardDate>2026-08-20+02:00</cbc:AwardDate></cac:TenderResult>
</ContractAwardNotice>`

func award(pairs ...string) []byte {
	return []byte(strings.NewReplacer(pairs...).Replace(sampleAward))
}

func TestParseReadsTheResultOfAnAwardNotice(t *testing.T) {
	t.Parallel()

	n, err := Parse(award())
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if n.Root != "ContractAwardNotice" || n.Title != "Eksempeltjeneste" || n.ProcedureType != "open" {
		t.Errorf("root %q title %q procedure %q", n.Root, n.Title, n.ProcedureType)
	}
	if !slices.Equal(n.BuyerNames, []string{"Eksempelkommune"}) || !slices.Equal(n.BuyerOrgNumbers, []string{"000000000"}) {
		t.Errorf("buyer = %v %v; the winner must not be taken for the buyer", n.BuyerNames, n.BuyerOrgNumbers)
	}
	if n.AwardedValue == nil || *n.AwardedValue != (Amount{Minor: 240000000, Currency: "NOK"}) {
		t.Errorf("awarded value = %+v, want 2 400 000 NOK", n.AwardedValue)
	}
	if want := time.Date(2026, time.August, 20, 0, 0, 0, 0, time.UTC); n.AwardDate == nil || !n.AwardDate.Equal(want) {
		t.Errorf("award date = %v, want the calendar day %v", n.AwardDate, want)
	}
	if len(n.Results) != 2 {
		t.Fatalf("results = %d, want 2", len(n.Results))
	}

	won := n.Results[0]
	if won.Lot != "LOT-0001" || won.Status != "selec-w" {
		t.Errorf("first result = lot %q status %q", won.Lot, won.Status)
	}
	if won.TendersReceived == nil || *won.TendersReceived != 7 {
		t.Errorf("tenders received = %v, want 7 (the t-sme count is not the tender count)", won.TendersReceived)
	}
	if len(won.Winners) != 1 || won.Winners[0] != (Winner{Name: "Fiktiv Leverandør AS", OrgNumber: "111111111"}) {
		t.Errorf("winners = %+v", won.Winners)
	}
	if won.Value == nil || *won.Value != (Amount{Minor: 240000000, Currency: "NOK"}) {
		t.Errorf("winning tender value = %+v", won.Value)
	}

	closed := n.Results[1]
	if closed.Lot != "LOT-0002" || closed.Status != "clos-nw" || len(closed.Winners) != 0 || closed.TendersReceived != nil {
		t.Errorf("closed result = %+v", closed)
	}
}

func TestParseReadsWeightedAwardCriteriaPerLot(t *testing.T) {
	t.Parallel()

	n, err := Parse(award())
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if len(n.Lots) != 2 {
		t.Fatalf("lots = %d, want 2", len(n.Lots))
	}
	got := n.Lots[0].AwardCriteria
	if len(got) != 2 {
		t.Fatalf("criteria = %+v, want 2", got)
	}
	if got[0].Type != "quality" || got[0].Name != "Kvalitet" || got[0].Weight == nil || *got[0].Weight != 60 {
		t.Errorf("first criterion = %+v", got[0])
	}
	if got[1].Type != "price" || got[1].Weight == nil || *got[1].Weight != 40 {
		t.Errorf("second criterion = %+v", got[1])
	}
	if len(n.Lots[1].AwardCriteria) != 0 {
		t.Errorf("a lot without criteria got %+v", n.Lots[1].AwardCriteria)
	}
}

func TestParseOnlyTakesAPercentageAsAWeight(t *testing.T) {
	t.Parallel()

	n, err := Parse(award(`listName="number-weight">per-exa</efbc:ParameterCode><efbc:ParameterNumeric>60`,
		`listName="number-weight">ord-imp</efbc:ParameterCode><efbc:ParameterNumeric>1`))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if w := n.Lots[0].AwardCriteria[0].Weight; w != nil {
		t.Errorf("an order of importance became a weight of %v", *w)
	}
}

func TestParseNeverKeepsContactPersonsOfAWinner(t *testing.T) {
	t.Parallel()

	n, err := Parse(award())
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if n.ContactPoints != 2 {
		t.Errorf("contact points = %d, want 2 (buyer and winner)", n.ContactPoints)
	}
	assertNoPersonalData(t, n, "Fiktiv Person", "Annen Fiktiv", "example.invalid", "+47 00000000")
}

// assertNoPersonalData fails when any of the given strings is in the parsed notice.
func assertNoPersonalData(t *testing.T, n Notice, personal ...string) {
	t.Helper()
	out, err := json.Marshal(n)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	for _, p := range personal {
		if strings.Contains(string(out), p) {
			t.Errorf("the parsed notice carries contact data of the notice")
		}
	}
}

func TestParseReportsAWinnerItCannotResolve(t *testing.T) {
	t.Parallel()

	for name, pairs := range map[string][]string{
		"unknown tender":           {`<efac:LotTender><cbc:ID schemeName="tender">TEN-0001</cbc:ID></efac:LotTender>` + "\n        <efac:ReceivedSubmissionsStatistics><efbc:StatisticsCode listName=\"received-submission-type\">t-sme", `<efac:LotTender><cbc:ID schemeName="tender">TEN-0009</cbc:ID></efac:LotTender>` + "\n        <efac:ReceivedSubmissionsStatistics><efbc:StatisticsCode listName=\"received-submission-type\">t-sme"},
		"unknown tendering party":  {`<efac:TenderingParty><cbc:ID schemeName="tendering-party">TPA-0001</cbc:ID></efac:TenderingParty>`, `<efac:TenderingParty><cbc:ID schemeName="tendering-party">TPA-0009</cbc:ID></efac:TenderingParty>`},
		"unknown organisation":     {`<efac:Tenderer><cbc:ID schemeName="organization">ORG-0002</cbc:ID>`, `<efac:Tenderer><cbc:ID schemeName="organization">ORG-0009</cbc:ID>`},
		"tender count not a count": {`<efbc:StatisticsNumeric>7</efbc:StatisticsNumeric>`, `<efbc:StatisticsNumeric>sju</efbc:StatisticsNumeric>`},
	} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			n, err := Parse(award(pairs...))
			if err != nil {
				t.Fatalf("Parse: %v", err)
			}
			if len(n.Issues) == 0 {
				t.Errorf("no issue reported")
			}
			if name == "tender count not a count" {
				if n.Results[0].TendersReceived != nil {
					t.Errorf("tender count = %v, want none", *n.Results[0].TendersReceived)
				}
				return
			}
			if len(n.Results[0].Winners) != 0 {
				t.Errorf("winners = %+v, want none: a winner is never guessed", n.Results[0].Winners)
			}
		})
	}
}

func TestParseRefusesAnAwardNoticeOfAnotherSDK(t *testing.T) {
	t.Parallel()

	_, err := Parse(award("eforms-sdk-1.14#", "eforms-sdk-1.12#"))
	if !errors.Is(err, ErrNoFieldMap) {
		t.Errorf("err = %v, want ErrNoFieldMap", err)
	}
}

func TestParseReadsTheProcedureTypeOfAContractNotice(t *testing.T) {
	t.Parallel()

	n, err := Parse(sample(`<cac:TenderingProcess>
    <cac:TenderSubmissionDeadlinePeriod>`, `<cac:TenderingProcess>
    <cbc:ProcedureCode listName="procurement-procedure-type">neg-w-call</cbc:ProcedureCode>
    <cac:TenderSubmissionDeadlinePeriod>`))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if n.ProcedureType != "neg-w-call" {
		t.Errorf("procedure = %q", n.ProcedureType)
	}
	if len(n.Results) != 0 || n.AwardedValue != nil || n.AwardDate != nil {
		t.Errorf("a contract notice got a result: %+v %v %v", n.Results, n.AwardedValue, n.AwardDate)
	}
}

// A lot won by two tenders (a framework with two suppliers) names both winners, and no single
// tender's value is shown as the lot's value.
func TestParseNamesEveryWinnerOfALotWithTwoTenders(t *testing.T) {
	t.Parallel()

	n, err := Parse(award(
		`<efac:LotTender><cbc:ID schemeName="tender">TEN-0001</cbc:ID></efac:LotTender>`+"\n        <efac:ReceivedSubmissionsStatistics>",
		`<efac:LotTender><cbc:ID schemeName="tender">TEN-0001</cbc:ID></efac:LotTender><efac:LotTender><cbc:ID schemeName="tender">TEN-0002</cbc:ID></efac:LotTender>`+"\n        <efac:ReceivedSubmissionsStatistics>",
		`<efac:SettledContract>
        <cbc:ID schemeName="contract">CON-0001</cbc:ID>
        <cbc:IssueDate>`,
		`<efac:LotTender><cbc:ID schemeName="tender">TEN-0002</cbc:ID><cac:LegalMonetaryTotal><cbc:PayableAmount currencyID="NOK">100</cbc:PayableAmount></cac:LegalMonetaryTotal><efac:TenderingParty><cbc:ID schemeName="tendering-party">TPA-0001</cbc:ID></efac:TenderingParty></efac:LotTender>
      <efac:SettledContract>
        <cbc:ID schemeName="contract">CON-0001</cbc:ID>
        <cbc:IssueDate>`))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	won := n.Results[0]
	if len(won.Winners) != 2 {
		t.Errorf("winners = %+v, want 2", won.Winners)
	}
	if won.Value != nil {
		t.Errorf("value = %+v, want none: one tender's value is not the lot's", won.Value)
	}
}

func TestParseRefusesAWeightOverOneHundredPercent(t *testing.T) {
	t.Parallel()

	n, err := Parse(award(`<efbc:ParameterNumeric>60</efbc:ParameterNumeric>`, `<efbc:ParameterNumeric>150</efbc:ParameterNumeric>`))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if w := n.Lots[0].AwardCriteria[0].Weight; w != nil || len(n.Issues) == 0 {
		t.Errorf("weight %v, issues %v: 150 %% must be an issue, not a weight", w, n.Issues)
	}
}
