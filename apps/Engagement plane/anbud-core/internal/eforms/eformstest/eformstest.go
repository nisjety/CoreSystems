// Package eformstest builds synthetic eForms notices for the tests of packages that consume the
// parser. Every name, number and address in them is invented, the e-mail domain is reserved, and
// the one contact person in each notice is there so that a test can show the person never comes
// out (PersonalMarkers). Nothing here is read from, or shaped by, a captured notice's content.
package eformstest

import (
	"bytes"
	"encoding/xml"
	"strconv"
	"strings"
	"time"
)

// PersonalMarkers are the contact details every fixture carries. A test of an output that must
// not hold personal data looks for these.
var PersonalMarkers = []string{"Fiktiv Person", "person@example.invalid", "+47 00 00 00 00"}

// Notice says what a fixture holds. The zero value is a valid notice for IT services in Oslo with
// no value, no deadline and no selection criteria.
type Notice struct {
	ID          string // the notice's UUID; a fixed one when empty
	ProcedureID string // the procedure's UUID; empty gives a notice with no procedure, which is never grouped with another
	SDK         string // "1.14" when empty
	// CustomizationID replaces the whole cbc:CustomizationID. Empty gives the Norwegian eu flavour of
	// SDK; "eforms-sdk-1.13" is a plain EU notice as TED publishes it.
	CustomizationID string
	Title           string
	Description     string
	CPV             string // 72212000 when empty
	NUTS            string // NO081 when empty
	ValueNOK        int64  // whole kroner; 0 leaves the value out
	Buyer           string // Eksempelkommune when empty
	BuyerOrg        string // 974760673 when empty
	// Deadline is the lot's deadline for tenders, and ParticipationDeadline its deadline for
	// requests to participate. A zero time leaves either out.
	Deadline              time.Time
	ParticipationDeadline time.Time
	Criteria              []string // selection criteria, one description each
	PointToDocuments      bool     // the notice says the criteria are in the tender documents
	// IssuedAt is when the notice was issued; a zero time leaves the issue date and time out.
	IssuedAt time.Time
	// PlannedNotice is the day a prior information notice plans its notice for (BT-127); a zero time
	// leaves it out.
	PlannedNotice time.Time
	// Change makes the notice a change notice: it gets a Changes section.
	Change *Change
}

// Change is the change section of a change notice, in the shape the captured ones have.
type Change struct {
	Replaces   string // the reference to the changed notice, "<uuid>-<version>" or a publication number
	ReasonCode string // update-add when empty
	Reason     string // the buyer's words, in Norwegian
	Sections   []string
}

func orDefault(s, def string) string {
	if strings.TrimSpace(s) == "" {
		return def
	}
	return s
}

func esc(s string) string {
	var b bytes.Buffer
	_ = xml.EscapeText(&b, []byte(s))
	return b.String()
}

func period(tag string, t time.Time) string {
	if t.IsZero() {
		return ""
	}
	t = t.UTC()
	return "<cac:" + tag + "><cbc:EndDate>" + t.Format("2006-01-02") + "</cbc:EndDate><cbc:EndTime>" + t.Format("15:04:05") + "Z</cbc:EndTime></cac:" + tag + ">"
}

// ContractNotice returns a ContractNotice of the given SDK version, flavour eu, laid out the way
// the captured notices are: the buyer's details in the organisation list of the extension block,
// document links, criteria and deadline on the lot.
func ContractNotice(n Notice) []byte { return build(n, "ContractNotice", "cn-standard", nil) }

// AwardNotice returns the same content as a ContractAwardNotice (type can-standard) with no
// result in it: a notice of a shape that is read, and is not a tender to bid on.
func AwardNotice(n Notice) []byte { return build(n, "ContractAwardNotice", "can-standard", nil) }

// PriorInformationNotice returns a PriorInformationNotice of type pin-only, laid out like the contract
// notice: the same project, buyer and lot, announcing a tender and inviting none.
func PriorInformationNotice(n Notice) []byte {
	return build(n, "PriorInformationNotice", "pin-only", nil)
}

// Award is the result an award fixture holds: one lot, one winner chosen. A zero field is left out.
type Award struct {
	WinnerName     string
	WinnerOrg      string
	Tenders        int       // the tenders received
	AwardDate      time.Time // the award day
	DurationMonths int       // the lot's contract period
}

// AwardWithResult returns a ContractAwardNotice whose lot was won by the award's winner, laid out as
// the captured award notices are: the result in the extension block, reached from the lot result
// through the tender and the tendering party to the winner in the organisation list.
func AwardWithResult(n Notice, a Award) []byte {
	return build(n, "ContractAwardNotice", "can-standard", &a)
}

func build(n Notice, root, noticeType string, a *Award) []byte {
	sdk := orDefault(n.SDK, "1.14")
	id := orDefault(n.ID, "00000000-0000-4000-8000-000000000001")
	folder := ""
	if n.ProcedureID != "" {
		folder = `<cbc:ContractFolderID>` + esc(n.ProcedureID) + `</cbc:ContractFolderID>`
	}
	result, winnerOrg, awardDate, lotPeriod := awardParts(a)
	buyer := esc(orDefault(n.Buyer, "Eksempelkommune"))
	org := esc(orDefault(n.BuyerOrg, "974760673"))
	issue, changes := issueAndChanges(n)
	value := ""
	if n.ValueNOK > 0 {
		value = `<cac:RequestedTenderTotal><cbc:EstimatedOverallContractAmount currencyID="NOK">` + strconv.FormatInt(n.ValueNOK, 10) + `.00</cbc:EstimatedOverallContractAmount></cac:RequestedTenderTotal>`
	}
	criteria := ""
	if len(n.Criteria) > 0 {
		var b strings.Builder
		b.WriteString(`<ext:UBLExtensions><ext:UBLExtension><ext:ExtensionContent><efext:EformsExtension>`)
		for _, c := range n.Criteria {
			b.WriteString(`<efac:SelectionCriteria><cbc:CalculationExpressionCode listName="usage">used</cbc:CalculationExpressionCode>` +
				`<cbc:CriterionTypeCode listName="selection-criterion">other</cbc:CriterionTypeCode>` +
				`<cbc:Name languageID="NOR">Krav</cbc:Name><cbc:Description languageID="NOR">` + esc(c) + `</cbc:Description></efac:SelectionCriteria>`)
		}
		b.WriteString(`</efext:EformsExtension></ext:ExtensionContent></ext:UBLExtension></ext:UBLExtensions>`)
		criteria = b.String()
	}
	grounds := ""
	for _, g := range []string{"exg-crim-fraud", "exg-sitn-bankr", "exg-pmt-bre-tax"} {
		grounds += `<cac:SpecificTendererRequirement><cbc:TendererRequirementTypeCode>` + g + `</cbc:TendererRequirementTypeCode></cac:SpecificTendererRequirement>`
	}
	if n.PointToDocuments {
		grounds += `<cac:SpecificTendererRequirement><cbc:TendererRequirementTypeCode>epo-procurement-document</cbc:TendererRequirementTypeCode></cac:SpecificTendererRequirement>`
	}

	doc := `<?xml version="1.0" encoding="UTF-8"?>
<` + root + ` xmlns="urn:oasis:names:specification:ubl:schema:xsd:` + root + `-2"
 xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2"
 xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
 xmlns:ext="urn:oasis:names:specification:ubl:schema:xsd:CommonExtensionComponents-2"
 xmlns:efac="http://data.europa.eu/p27/eforms-ubl-extension-aggregate-components/1"
 xmlns:efbc="http://data.europa.eu/p27/eforms-ubl-extension-basic-components/1"
 xmlns:efext="http://data.europa.eu/p27/eforms-ubl-extensions/1">
  <ext:UBLExtensions><ext:UBLExtension><ext:ExtensionContent><efext:EformsExtension>
    ` + result + changes + `
    <efac:NoticeSubType><cbc:SubTypeCode listName="notice-subtype">16</cbc:SubTypeCode></efac:NoticeSubType>
    <efac:Organizations><efac:Organization><efac:Company>
      <cac:PartyIdentification><cbc:ID>ORG-0001</cbc:ID></cac:PartyIdentification>
      <cac:PartyLegalEntity><cbc:CompanyID>` + org + `</cbc:CompanyID></cac:PartyLegalEntity>
      <cac:PartyName><cbc:Name>` + buyer + `</cbc:Name></cac:PartyName>
      <cac:Contact><cbc:Name>Fiktiv Person</cbc:Name><cbc:Telephone>+47 00 00 00 00</cbc:Telephone><cbc:ElectronicMail>person@example.invalid</cbc:ElectronicMail></cac:Contact>
    </efac:Company></efac:Organization>` + winnerOrg + `</efac:Organizations>
  </efext:EformsExtension></ext:ExtensionContent></ext:UBLExtension></ext:UBLExtensions>
  <cbc:UBLVersionID>2.3</cbc:UBLVersionID>
  <cbc:CustomizationID>` + esc(orDefault(n.CustomizationID, "eforms-sdk-"+sdk+"#extended#urn:fdc:anskaffelser.no:2023:eforms:eu")) + `</cbc:CustomizationID>
  <cbc:ID>` + esc(id) + `</cbc:ID>
  <cbc:VersionID>01</cbc:VersionID>
  ` + folder + `
  <cbc:NoticeTypeCode>` + noticeType + `</cbc:NoticeTypeCode>` + issue + `
  <cbc:RegulatoryDomain>32014L0024</cbc:RegulatoryDomain>
  <cac:ProcurementProject>
    <cbc:Name>` + esc(orDefault(n.Title, "Eksempeltjeneste")) + `</cbc:Name>
    <cbc:Description>` + esc(orDefault(n.Description, "Fiktiv beskrivelse av en tjeneste.")) + `</cbc:Description>
    <cbc:ProcurementTypeCode>services</cbc:ProcurementTypeCode>
    ` + value + `
    <cac:MainCommodityClassification><cbc:ItemClassificationCode listName="cpv">` + esc(orDefault(n.CPV, "72212000")) + `</cbc:ItemClassificationCode></cac:MainCommodityClassification>
    <cac:RealizedLocation><cac:Address><cbc:CountrySubentityCode>` + esc(orDefault(n.NUTS, "NO081")) + `</cbc:CountrySubentityCode></cac:Address></cac:RealizedLocation>
  </cac:ProcurementProject>
  <cac:TenderingTerms><cac:TendererQualificationRequest>` + grounds + `</cac:TendererQualificationRequest></cac:TenderingTerms>
  <cac:TenderingProcess><cbc:ProcedureCode listName="procurement-procedure-type">open</cbc:ProcedureCode></cac:TenderingProcess>
  <cac:ContractingParty><cac:Party><cac:PartyIdentification><cbc:ID>ORG-0001</cbc:ID></cac:PartyIdentification></cac:Party></cac:ContractingParty>
  <cac:ProcurementProjectLot>
    <cbc:ID schemeName="Lot">LOT-0001</cbc:ID>
    <cac:ProcurementProject><cbc:Name>Delkontrakt 1</cbc:Name>` + lotPeriod + `</cac:ProcurementProject>
    <cac:TenderingTerms>
      ` + criteria + `
      <cac:CallForTendersDocumentReference><cac:Attachment><cac:ExternalReference><cbc:URI>https://example.invalid/docs/1</cbc:URI></cac:ExternalReference></cac:Attachment></cac:CallForTendersDocumentReference>
    </cac:TenderingTerms>
    <cac:TenderingProcess>` + period("TenderSubmissionDeadlinePeriod", n.Deadline) + period("ParticipationRequestReceptionPeriod", n.ParticipationDeadline) + `</cac:TenderingProcess>
  </cac:ProcurementProjectLot>
  ` + awardDate + `
</` + root + `>`
	return []byte(doc)
}

// awardParts are the pieces an award adds to a notice: the result, the winner in the organisation
// list, the award day and the lot's contract period. All are empty without an award.
func awardParts(a *Award) (result, winnerOrg, awardDate, lotPeriod string) {
	if a == nil {
		return "", "", "", ""
	}
	tenders := ""
	if a.Tenders > 0 {
		tenders = `<efac:ReceivedSubmissionsStatistics><efbc:StatisticsCode listName="received-submission-type">tenders</efbc:StatisticsCode><efbc:StatisticsNumeric>` +
			strconv.Itoa(a.Tenders) + `</efbc:StatisticsNumeric></efac:ReceivedSubmissionsStatistics>`
	}
	result = `<efac:NoticeResult>
      <efac:LotResult><cbc:ID>RES-0001</cbc:ID><cbc:TenderResultCode listName="winner-selection-status">selec-w</cbc:TenderResultCode>
        <efac:LotTender><cbc:ID>TEN-0001</cbc:ID></efac:LotTender>` + tenders + `
        <efac:TenderLot><cbc:ID schemeName="Lot">LOT-0001</cbc:ID></efac:TenderLot></efac:LotResult>
      <efac:LotTender><cbc:ID>TEN-0001</cbc:ID><efac:TenderingParty><cbc:ID>TPA-0001</cbc:ID></efac:TenderingParty><efac:TenderLot><cbc:ID schemeName="Lot">LOT-0001</cbc:ID></efac:TenderLot></efac:LotTender>
      <efac:TenderingParty><cbc:ID>TPA-0001</cbc:ID><efac:Tenderer><cbc:ID>ORG-0002</cbc:ID></efac:Tenderer></efac:TenderingParty>
    </efac:NoticeResult>`
	winnerOrg = `<efac:Organization><efac:Company>
      <cac:PartyIdentification><cbc:ID>ORG-0002</cbc:ID></cac:PartyIdentification>
      <cac:PartyLegalEntity><cbc:CompanyID>` + esc(a.WinnerOrg) + `</cbc:CompanyID></cac:PartyLegalEntity>
      <cac:PartyName><cbc:Name>` + esc(a.WinnerName) + `</cbc:Name></cac:PartyName>
    </efac:Company></efac:Organization>`
	if !a.AwardDate.IsZero() {
		awardDate = `<cac:TenderResult><cbc:AwardDate>` + a.AwardDate.UTC().Format("2006-01-02") + `Z</cbc:AwardDate></cac:TenderResult>`
	}
	if a.DurationMonths > 0 {
		lotPeriod = `<cac:PlannedPeriod><cbc:DurationMeasure unitCode="MONTH">` + strconv.Itoa(a.DurationMonths) + `</cbc:DurationMeasure></cac:PlannedPeriod>`
	}
	return result, winnerOrg, awardDate, lotPeriod
}

// issueAndChanges are the pieces a fixture adds when it is issued at a time or is a change notice.
func issueAndChanges(n Notice) (issue, changes string) {
	if !n.IssuedAt.IsZero() {
		u := n.IssuedAt.UTC()
		issue = `<cbc:IssueDate>` + u.Format("2006-01-02") + `Z</cbc:IssueDate><cbc:IssueTime>` + u.Format("15:04:05") + `Z</cbc:IssueTime>`
	}
	if !n.PlannedNotice.IsZero() {
		issue += `<cbc:PlannedDate>` + n.PlannedNotice.UTC().Format("2006-01-02") + `Z</cbc:PlannedDate>`
	}
	if c := n.Change; c != nil {
		var b strings.Builder
		b.WriteString(`<efac:Changes>`)
		if c.Replaces != "" {
			b.WriteString(`<efbc:ChangedNoticeIdentifier>` + esc(c.Replaces) + `</efbc:ChangedNoticeIdentifier>`)
		}
		for _, s := range c.Sections {
			b.WriteString(`<efac:Change><efac:ChangedSection><efbc:ChangedSectionIdentifier>` + esc(s) + `</efbc:ChangedSectionIdentifier></efac:ChangedSection></efac:Change>`)
		}
		b.WriteString(`<efac:ChangeReason><cbc:ReasonCode listName="change-corrig-justification">` + esc(orDefault(c.ReasonCode, "update-add")) + `</cbc:ReasonCode>`)
		if c.Reason != "" {
			b.WriteString(`<efbc:ReasonDescription languageID="NOR">` + esc(c.Reason) + `</efbc:ReasonDescription>`)
		}
		b.WriteString(`</efac:ChangeReason></efac:Changes>`)
		changes = b.String()
	}
	return issue, changes
}
