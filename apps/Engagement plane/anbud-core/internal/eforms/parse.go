package eforms

import (
	"encoding/xml"
	"regexp"
	"strconv"
	"strings"
	"time"
)

// shapeKey selects the field map of a notice: its root element, its SDK version and its flavour.
type shapeKey struct {
	root, sdk string
	flavour   Flavour
}

// extractor reads one shape's fields from a tree into a notice and marks what it read.
type extractor func(root *node, n *Notice, r *reader)

// fieldMaps are the shapes this code reads. Each was written from the eForms UBL paths of its SDK
// and checked against the captured corpus by counts and element names only
// (TestParseCapturedNoticesCountsOnly). A shape not listed here is refused with ErrNoFieldMap.
//
// The 1.13 contract notice shares the 1.14 extractor: the paths this code reads are the same in both,
// and the five captured 1.13 notices read as completely as the 1.14 ones (counts, checked against an
// independent survey of the same files, in docs/SOURCES.md). Only the eu flavour of 1.13 was seen.
var fieldMaps = map[shapeKey]extractor{
	{"ContractNotice", "1.14", FlavourEU}:        contractNotice,
	{"ContractNotice", "1.14", FlavourNationalE}: contractNotice,
	{"ContractNotice", "1.13", FlavourEU}:        contractNotice,
	{"ContractAwardNotice", "1.14", FlavourEU}:   contractAwardNotice114,
	// Plain EU eForms, as TED publishes every notice, Norwegian ones included: the same UBL paths
	// without a national suffix (TestPlainTEDNoticesReadLikeTheirNorwegianFlavour).
	{"ContractNotice", "1.14", flavourStandard}:      contractNotice,
	{"ContractNotice", "1.13", flavourStandard}:      contractNotice,
	{"ContractAwardNotice", "1.14", flavourStandard}: contractAwardNotice114,
	{"ContractAwardNotice", "1.13", flavourStandard}: contractAwardNotice114,
	// Doffin's remaining shapes, checked against Doffin's own search metadata and TED's
	// (TestEveryHeldDoffinShapeReads, docs/SOURCES.md).
	{"ContractNotice", "1.13", FlavourNationalE}:      contractNotice,
	{"ContractNotice", "1.13", FlavourNational}:       contractNotice,
	{"ContractAwardNotice", "1.13", FlavourEU}:        contractAwardNotice114,
	{"ContractAwardNotice", "1.14", FlavourNationalE}: contractAwardNotice114,
	{"ContractAwardNotice", "1.13", FlavourNationalE}: contractAwardNotice114,
	{"ContractAwardNotice", "1.13", FlavourNational}:  contractAwardNotice114,
	// SDK 1.14 in the national flavour: mapped by analogy, never seen. Neither the 1 056 held notices
	// nor about 50 sampled from Doffin across April to October 2026 held one (1.14 came as eu or
	// national-e, national as 1.13). The extractors read 1.14 eu and national-e and 1.13 national, which
	// differ from it only in the suffix (TestSDK114NationalReadsLikeItsNeighbours).
	{"ContractNotice", "1.14", FlavourNational}:      contractNotice,
	{"ContractAwardNotice", "1.14", FlavourNational}: contractAwardNotice114,
	// Prior information notices: the contract notice's paths (TestPriorInformationNoticesReadLikeContractNotices).
	{"PriorInformationNotice", "1.14", FlavourEU}:        contractNotice,
	{"PriorInformationNotice", "1.13", FlavourEU}:        contractNotice,
	{"PriorInformationNotice", "1.14", FlavourNationalE}: contractNotice,
	{"PriorInformationNotice", "1.13", FlavourNationalE}: contractNotice,
	{"PriorInformationNotice", "1.14", flavourStandard}:  contractNotice,
	{"PriorInformationNotice", "1.13", flavourStandard}:  contractNotice,
	{"PriorInformationNotice", "1.13", FlavourNational}:  contractNotice,
	{"PriorInformationNotice", "1.14", FlavourNational}:  contractNotice, // by analogy, as above
}

// flavourStandard keys the field maps of plain EU eForms: a notice another country's buyer sent to
// TED, whose CustomizationID is the SDK and nothing else. It is a map key only; Header.Flavour stays
// empty for such a notice.
const flavourStandard Flavour = "standard"

// mapFlavour is the flavour a notice's field map is looked up by. An empty flavour means plain EU
// eForms only when the CustomizationID is exactly "eforms-sdk-<version>": a suffix this code does not
// know (another country's extension) keeps it empty, and empty has no field map.
func mapFlavour(h Header) Flavour {
	if h.Flavour == "" && h.SDKVersion != "" && h.CustomizationID == "eforms-sdk-"+h.SDKVersion {
		return flavourStandard
	}
	return h.Flavour
}

// headerChildren are the direct children of the root that Sniff reads as the header, and the
// signature block, which carries no business data. The extension block is read by path, so that
// what else it holds is reported rather than hidden.
var headerChildren = map[string]bool{
	"UBLVersionID": true, "CustomizationID": true, "ID": true, "VersionID": true,
	"ContractFolderID": true, "NoticeTypeCode": true, "Signature": true,
}

// The shapes of the values the parser reads. A value that does not match is an issue, not a value.
var (
	cpvCode  = regexp.MustCompile(`^\d{8}(?:-\d)?$`)
	nutsCode = regexp.MustCompile(`^[A-Z]{2}[A-Z0-9]{1,4}$`)
	// registryID is what an organisation identifier looks like in any country's registry: letters,
	// digits, and dots, dashes and slashes between them (556000-0000, FI1234567-8). A registry
	// number also has a digit, which free text with its spaces removed does not. It is not a check
	// that the number is valid, only that it is an identifier and not free text or markup.
	registryID = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9./-]{0,63}$`)
	moneyValue = regexp.MustCompile(`^(\d{1,15})(?:\.(\d{1,2}))?$`)
)

// The layouts of the dates and times a notice states. A date may carry its zone, which is not
// needed for the calendar day; a time must carry one, because the hour means nothing without it.
// Go accepts fractional seconds after the seconds field whatever the layout says.
const (
	zonedDate  = "2006-01-02Z07:00"
	isoDate    = "2006-01-02"
	zonedClock = "15:04:05Z07:00"
	bareClock  = "15:04:05"
)

// Parse reads a notice into the parsed model. It sniffs the header first, so a document that is
// not an eForms notice, or is over a limit, is refused the same way. Then it picks the field map
// for the notice's root, SDK version and flavour, and refuses with ErrNoFieldMap if there is none.
// Only the values a field map reads are set; the rest is in Unmapped.
func Parse(data []byte) (Notice, error) {
	h, err := Sniff(data)
	if err != nil {
		return Notice{}, err
	}
	extract, ok := fieldMaps[shapeKey{root: h.Root, sdk: h.SDKVersion, flavour: mapFlavour(h)}]
	if !ok {
		return Notice{Header: h}, ErrNoFieldMap
	}
	root, err := buildTree(data)
	if err != nil {
		return Notice{}, err
	}
	r := newReader(root)
	for _, c := range root.children {
		if headerChildren[c.name.Local] {
			r.consume(c)
		}
	}
	n := Notice{Header: h}
	extract(root, &n, r)
	n.Unmapped = unmapped(root, r.visited)
	return n, nil
}

// contractNotice reads a tender notice of eForms SDK 1.14 (the eu and national-e flavours) or 1.13
// (eu). Every path is an eForms path of those SDKs, and the namespaces are the ones the captured
// notices use. A value a notice carries somewhere else is not guessed at: it shows up in Unmapped.
func contractNotice(root *node, n *Notice, r *reader) {
	procedure114(root, n, r)
	n.Change = r.changes(root, &n.Issues)
	// A prior information notice says when its notice is planned; a contract notice has no such date.
	n.PlannedNoticeDate = r.dateAt(root, &n.Issues, "planned notice", cbc("PlannedDate"))
}

// procedure114 reads what every notice of SDK 1.14 states about the procedure: the project, the
// buyer, the procedure type and the lots. The parts only one kind of notice has are read by that
// notice's field map. It returns the organisation list, read once, so that a field map can resolve
// other parties without counting the contacts twice.
func procedure114(root *node, n *Notice, r *reader) map[string]organisation {
	r.walk(root, ext("UBLExtensions"), ext("UBLExtension"), ext("ExtensionContent"),
		efext("EformsExtension"), efac("NoticeSubType"), cbc("SubTypeCode"))
	n.ProcedureType = firstText(r.walk(root, cac("TenderingProcess"), cbc("ProcedureCode")))
	n.IssuedAt = r.issuedAt(root, &n.Issues)
	n.ContractingSystems = r.contractingSystems(root)

	if project := firstNode(r.walk(root, cac("ProcurementProject"))); project != nil {
		n.Title = firstText(r.walk(project, cbc("Name")))
		n.Description = firstText(r.walk(project, cbc("Description")))
		n.ProcurementType = firstText(r.walk(project, cbc("ProcurementTypeCode")))
		n.EstimatedValue = r.amountAt(project, &n.Issues, "procurement",
			cac("RequestedTenderTotal"), cbc("EstimatedOverallContractAmount"))
		n.CPVMain = r.coded(project, &n.Issues, "procurement", "a CPV code is not 8 digits, optionally with a check digit",
			cpvCode, cac("MainCommodityClassification"), cbc("ItemClassificationCode"))
		n.CPVAdditional = r.coded(project, &n.Issues, "procurement", "a CPV code is not 8 digits, optionally with a check digit",
			cpvCode, cac("AdditionalCommodityClassification"), cbc("ItemClassificationCode"))
		n.NUTS = r.coded(project, &n.Issues, "procurement", "a place code does not look like NUTS",
			nutsCode, cac("RealizedLocation"), cac("Address"), cbc("CountrySubentityCode"))
	}

	n.Deadline = r.deadlineAt(root, &n.Issues, "tender", cac("TenderingProcess"), cac("TenderSubmissionDeadlinePeriod"))
	n.ParticipationDeadline = r.deadlineAt(root, &n.Issues, "participation", cac("TenderingProcess"), cac("ParticipationRequestReceptionPeriod"))
	n.Documents = append(n.Documents, r.documents(root, "", &n.Issues)...)
	n.Requirements = append(n.Requirements, r.requirements(root, "", "tender", &n.Issues)...)

	organisations := r.organisations(root, n)
	for _, party := range r.walk(root, cac("ContractingParty"), cac("Party")) {
		key := firstText(r.walk(party, cac("PartyIdentification"), cbc("ID")))
		org, ok := organisations[key]
		if !ok {
			n.Issues = append(n.Issues, "buyer: a party refers to no organisation in the notice")
			continue
		}
		if org.name != "" {
			n.BuyerNames = append(n.BuyerNames, org.name)
		}
		if org.number != "" {
			n.BuyerOrgNumbers = append(n.BuyerOrgNumbers, org.number)
		}
	}

	for i, lot := range r.walk(root, cac("ProcurementProjectLot")) {
		label := "lot " + strconv.Itoa(i+1)
		lotID := firstText(r.walk(lot, cbc("ID")))
		item := Lot{ID: lotID}
		if project := firstNode(r.walk(lot, cac("ProcurementProject"))); project != nil {
			item.Title = firstText(r.walk(project, cbc("Name")))
			item.CPVMain = r.coded(project, &n.Issues, label, "a CPV code is not 8 digits, optionally with a check digit",
				cpvCode, cac("MainCommodityClassification"), cbc("ItemClassificationCode"))
			item.Value = r.amountAt(project, &n.Issues, label, cac("RequestedTenderTotal"), cbc("EstimatedOverallContractAmount"))
			item.Period = r.contractPeriod(project, label, &n.Issues)
			item.Renewals = r.renewals(project, label, &n.Issues)
		}
		item.Deadline = r.deadlineAt(lot, &n.Issues, label, cac("TenderingProcess"), cac("TenderSubmissionDeadlinePeriod"))
		item.ParticipationDeadline = r.deadlineAt(lot, &n.Issues, label+" participation", cac("TenderingProcess"), cac("ParticipationRequestReceptionPeriod"))
		n.Documents = append(n.Documents, r.documents(lot, lotID, &n.Issues)...)
		n.Requirements = append(n.Requirements, r.requirements(lot, lotID, label, &n.Issues)...)
		n.ContractingSystems = append(n.ContractingSystems, r.contractingSystems(lot)...)
		item.AwardCriteria = r.awardCriteria(lot, label, &n.Issues)
		n.SelectionCriteria = append(n.SelectionCriteria, r.selectionCriteria(lot, lotID)...)
		n.Lots = append(n.Lots, item)
	}
	return organisations
}

// organisation is one entry of the organisation list: the buyer, or another party of the notice.
type organisation struct {
	name   string
	number string // the registry identifier with its spaces removed, or "" when not given or not an identifier
}

// organisations reads the organisation list of the extension block. Each organisation is a
// company with an identifier, which a party of the notice refers to by it; an organisation number,
// a name and any contacts. The contacts are counted into the notice and never copied.
func (r *reader) organisations(root *node, n *Notice) map[string]organisation {
	found := map[string]organisation{}
	for _, org := range r.walk(root, ext("UBLExtensions"), ext("UBLExtension"), ext("ExtensionContent"),
		efext("EformsExtension"), efac("Organizations"), efac("Organization")) {
		company := firstNode(r.walk(org, efac("Company")))
		if company == nil {
			continue
		}
		key := firstText(r.walk(company, cac("PartyIdentification"), cbc("ID")))
		number := strings.ReplaceAll(firstText(r.walk(company, cac("PartyLegalEntity"), cbc("CompanyID"))), " ", "")
		if number != "" && !isRegistryID(number) {
			n.Issues = append(n.Issues, "organisation: an identifier does not look like a registry number")
			number = ""
		}
		found[key] = organisation{name: firstText(r.walk(company, cac("PartyName"), cbc("Name"))), number: number}
		n.ContactPoints += r.countDescendants(company, cac("Contact"))
	}
	return found
}

// documents reads the links of the tender documents under n, which is the notice or a lot.
// A link that is not http or https is an issue, and not kept.
func (r *reader) documents(n *node, lot string, issues *[]string) []DocumentRef {
	var out []DocumentRef
	for _, uri := range r.walk(n, cac("TenderingTerms"), cac("CallForTendersDocumentReference"),
		cac("Attachment"), cac("ExternalReference"), cbc("URI")) {
		link := uri.textValue()
		if !strings.HasPrefix(link, "https://") && !strings.HasPrefix(link, "http://") {
			*issues = append(*issues, "a document link is not http or https")
			continue
		}
		out = append(out, DocumentRef{Lot: lot, URI: link, Access: AccessUnknown})
	}
	return out
}

// requirements reads the qualification requirements under n, which is the notice or a lot. Each
// has the code the notice gives it and the description as written. A requirement with no code is
// kept, and an issue says so.
func (r *reader) requirements(n *node, lot, label string, issues *[]string) []Requirement {
	var out []Requirement
	for _, req := range r.walk(n, cac("TenderingTerms"), cac("TendererQualificationRequest"), cac("SpecificTendererRequirement")) {
		item := Requirement{
			Lot:  lot,
			Type: firstText(r.walk(req, cbc("TendererRequirementTypeCode"))),
			Text: firstText(r.walk(req, cbc("Description"))),
		}
		if item.Type == "" {
			*issues = append(*issues, label+": a qualification requirement has no type code")
		}
		out = append(out, item)
	}
	return out
}

// amountAt reads the amount at the steps below n. A value that is not a plain decimal, or has no
// currency, is an issue and gives no amount.
func (r *reader) amountAt(n *node, issues *[]string, label string, steps ...xml.Name) *Amount {
	amount := firstNode(r.walk(n, steps...))
	if amount == nil {
		return nil
	}
	m := moneyValue.FindStringSubmatch(amount.textValue())
	if m == nil {
		*issues = append(*issues, label+": an amount is not a plain decimal")
		return nil
	}
	currency := amount.attr("currencyID")
	if currency == "" {
		*issues = append(*issues, label+": an amount has no currency")
		return nil
	}
	major, _ := strconv.ParseInt(m[1], 10, 64)
	var minor int64
	if frac := m[2]; frac != "" {
		if len(frac) == 1 {
			frac += "0"
		}
		minor, _ = strconv.ParseInt(frac, 10, 64)
	}
	return &Amount{Minor: major*100 + minor, Currency: currency}
}

// coded returns the value of each element reached that matches the pattern. One that does not is
// an issue with the text given, and no value.
func (r *reader) coded(n *node, issues *[]string, label, what string, pattern *regexp.Regexp, steps ...xml.Name) []string {
	var out []string
	for _, c := range r.walk(n, steps...) {
		v := c.textValue()
		if !pattern.MatchString(v) {
			*issues = append(*issues, label+": "+what)
			continue
		}
		out = append(out, v)
	}
	return out
}

// deadlineAt reads the submission period at the steps below n. The calendar day comes from the
// date, whatever zone it carries. The hour and its zone come from the time: a time with no zone
// gives no deadline, and so does a missing date or time. The parser does not choose a zone or an
// hour for the notice.
func (r *reader) deadlineAt(n *node, issues *[]string, label string, steps ...xml.Name) *time.Time {
	period := firstNode(r.walk(n, steps...))
	if period == nil {
		return nil
	}
	day := firstNode(r.walk(period, cbc("EndDate")))
	clock := firstNode(r.walk(period, cbc("EndTime")))
	if day == nil {
		*issues = append(*issues, label+": deadline has no end date")
		return nil
	}
	date, err := time.Parse(zonedDate, day.textValue())
	if err != nil {
		date, err = time.Parse(isoDate, day.textValue())
	}
	if err != nil {
		*issues = append(*issues, label+": deadline date is not a date")
		return nil
	}
	if clock == nil {
		*issues = append(*issues, label+": deadline has no time")
		return nil
	}
	at, err := time.Parse(zonedClock, clock.textValue())
	if err != nil {
		if _, bare := time.Parse(bareClock, clock.textValue()); bare == nil {
			*issues = append(*issues, label+": deadline time has no zone")
		} else {
			*issues = append(*issues, label+": deadline time is not a time")
		}
		return nil
	}
	t := time.Date(date.Year(), date.Month(), date.Day(), at.Hour(), at.Minute(), at.Second(), 0, at.Location())
	return &t
}

// norwegian are the language codes a Norwegian text is given under.
var norwegian = map[string]bool{"NOR": true, "NOB": true, "NNO": true}

// selectionCriteria reads the selection criteria of a lot, from its tendering terms' extension
// block. A criterion marked "not-used" is read and left out.
func (r *reader) selectionCriteria(lot *node, lotID string) []SelectionCriterion {
	var out []SelectionCriterion
	for _, c := range r.walk(lot, cac("TenderingTerms"), ext("UBLExtensions"), ext("UBLExtension"),
		ext("ExtensionContent"), efext("EformsExtension"), efac("SelectionCriteria")) {
		if firstText(r.walk(c, cbc("CalculationExpressionCode"))) == "not-used" {
			r.consume(c)
			continue
		}
		kind := firstText(r.walk(c, cbc("TendererRequirementTypeCode")))
		if kind == "" {
			kind = firstText(r.walk(c, cbc("CriterionTypeCode")))
		}
		out = append(out, SelectionCriterion{
			Lot:         lotID,
			Type:        kind,
			Name:        preferNorwegian(r.walk(c, cbc("Name"))),
			Description: preferNorwegian(r.walk(c, cbc("Description"))),
		})
	}
	return out
}

// preferNorwegian returns the text given in Norwegian, or the first text when none is.
func preferNorwegian(nodes []*node) string {
	for _, n := range nodes {
		if norwegian[strings.ToUpper(n.attr("languageID"))] {
			return n.textValue()
		}
	}
	return firstText(nodes)
}

// isRegistryID reports whether s is shaped like an organisation identifier of some registry.
func isRegistryID(s string) bool {
	return registryID.MatchString(s) && strings.ContainsAny(s, "0123456789")
}

// contractingSystems reads the SDK fields by namespace and list name, never from title text.
func (r *reader) contractingSystems(scope *node) []ContractingSystem {
	out := []ContractingSystem{}
	for _, code := range r.walk(scope, cac("TenderingProcess"), cac("ContractingSystem"), cbc("ContractingSystemTypeCode")) {
		kind := code.attr("listName")
		if kind == "framework-agreement" || kind == "dps-usage" {
			out = append(out, ContractingSystem{Kind: kind, Code: code.textValue()})
		}
	}
	return out
}
