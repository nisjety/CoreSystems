package eforms

import (
	"encoding/xml"
	"regexp"
	"strconv"
	"time"
)

// The shapes of the numbers an award notice states. A count is whole; a weight in percent is whole
// or has zero decimals, and is at most 100.
var (
	countValue  = regexp.MustCompile(`^\d{1,6}$`)
	weightValue = regexp.MustCompile(`^(\d{1,3})(?:\.0+)?$`)
)

// The codes the parser acts on. Each is a value of an eForms code list: received-submission-type,
// number-weight.
const (
	statisticTenders = "tenders"
	weightPercent    = "per-exa"
)

// contractAwardNotice114 reads a result notice of eForms SDK 1.14, the eu flavour: the procedure
// as contractNotice reads it, then the award date and the result. The result is in the
// extension block, and a winner is reached by references only: lot result → tender → tendering
// party → tenderer → organisation. A reference that leads nowhere is an issue and gives no winner.
func contractAwardNotice114(root *node, n *Notice, r *reader) {
	organisations := procedure114(root, n, r)
	n.AwardDate = r.dateAt(root, &n.Issues, "result", cac("TenderResult"), cbc("AwardDate"))

	result := firstNode(r.walk(root, ext("UBLExtensions"), ext("UBLExtension"), ext("ExtensionContent"),
		efext("EformsExtension"), efac("NoticeResult")))
	if result == nil {
		return
	}
	n.AwardedValue = r.amountAt(result, &n.Issues, "result", cbc("TotalAmount"))
	tenders := r.lotTenders(result, &n.Issues)
	parties := r.tenderingParties(result)
	contracts := r.settledContracts(result, &n.Issues)

	for i, lotResult := range r.walk(result, efac("LotResult")) {
		label := "result " + strconv.Itoa(i+1)
		item := LotResult{
			Lot:             firstText(r.walk(lotResult, efac("TenderLot"), cbc("ID"))),
			Status:          firstText(r.walk(lotResult, cbc("TenderResultCode"))),
			TendersReceived: r.tenderCount(lotResult, label, &n.Issues),
		}
		if id := firstText(r.walk(lotResult, efac("SettledContract"), cbc("ID"))); id != "" {
			signed, ok := contracts[id]
			if !ok {
				n.Issues = append(n.Issues, label+": a result refers to no contract in the notice")
			}
			item.ContractSigned = signed
		}
		tenderIDs := r.walk(lotResult, efac("LotTender"), cbc("ID"))
		for _, id := range tenderIDs {
			tender, ok := tenders[id.textValue()]
			if !ok {
				n.Issues = append(n.Issues, label+": a result refers to no tender in the notice")
				continue
			}
			if len(tenderIDs) == 1 {
				item.Value = tender.value
			}
			tenderers, ok := parties[tender.party]
			if !ok {
				n.Issues = append(n.Issues, label+": a tender refers to no tendering party in the notice")
				continue
			}
			for _, key := range tenderers {
				org, ok := organisations[key]
				if !ok {
					n.Issues = append(n.Issues, label+": a tenderer refers to no organisation in the notice")
					continue
				}
				item.Winners = append(item.Winners, Winner{Name: org.name, OrgNumber: org.number})
			}
		}
		n.Results = append(n.Results, item)
	}
}

// lotTender is a tender of the result: the tendering party that made it and its value.
type lotTender struct {
	party string
	value *Amount
}

// lotTenders reads the tenders of the result by their identifiers.
func (r *reader) lotTenders(result *node, issues *[]string) map[string]lotTender {
	found := map[string]lotTender{}
	for _, tender := range r.walk(result, efac("LotTender")) {
		id := firstText(r.walk(tender, cbc("ID")))
		found[id] = lotTender{
			party: firstText(r.walk(tender, efac("TenderingParty"), cbc("ID"))),
			value: r.amountAt(tender, issues, "tender", cac("LegalMonetaryTotal"), cbc("PayableAmount")),
		}
		r.walk(tender, efac("TenderLot"), cbc("ID"))
	}
	return found
}

// tenderingParties reads the tendering parties of the result: for each, the organisations that
// tendered, by the keys of the organisation list. Subcontractors are not tenderers and are not read.
func (r *reader) tenderingParties(result *node) map[string][]string {
	found := map[string][]string{}
	for _, party := range r.walk(result, efac("TenderingParty")) {
		id := firstText(r.walk(party, cbc("ID")))
		var keys []string
		for _, tenderer := range r.walk(party, efac("Tenderer"), cbc("ID")) {
			keys = append(keys, tenderer.textValue())
		}
		found[id] = keys
	}
	return found
}

// tenderCount reads how many tenders a lot received. Only the statistic coded "tenders" is the
// tender count; the others (from small firms, by e-mail, from abroad) are parts of it and stay
// unread. A count that is not a whole number is an issue and gives no count.
func (r *reader) tenderCount(lotResult *node, label string, issues *[]string) *int {
	for _, stat := range r.walk(lotResult, efac("ReceivedSubmissionsStatistics")) {
		if firstText(r.walk(stat, efbc("StatisticsCode"))) != statisticTenders {
			continue
		}
		value := firstText(r.walk(stat, efbc("StatisticsNumeric")))
		if !countValue.MatchString(value) {
			*issues = append(*issues, label+": the tender count is not a whole number")
			return nil
		}
		count, _ := strconv.Atoi(value)
		return &count
	}
	return nil
}

// awardCriteria reads the criteria a lot is awarded on. The weight is read only when its parameter
// is coded as an exact percentage; any other parameter (an order of importance, points, a range)
// gives no weight, so a rank is never shown as a share.
func (r *reader) awardCriteria(lot *node, label string, issues *[]string) []AwardCriterion {
	var out []AwardCriterion
	for _, c := range r.walk(lot, cac("TenderingTerms"), cac("AwardingTerms"), cac("AwardingCriterion"), cac("SubordinateAwardingCriterion")) {
		item := AwardCriterion{
			Type:        firstText(r.walk(c, cbc("AwardingCriterionTypeCode"))),
			Name:        firstText(r.walk(c, cbc("Name"))),
			Description: firstText(r.walk(c, cbc("Description"))),
		}
		for _, p := range r.walk(c, ext("UBLExtensions"), ext("UBLExtension"), ext("ExtensionContent"),
			efext("EformsExtension"), efac("AwardCriterionParameter")) {
			if firstText(r.walk(p, efbc("ParameterCode"))) != weightPercent {
				continue
			}
			m := weightValue.FindStringSubmatch(firstText(r.walk(p, efbc("ParameterNumeric"))))
			weight := -1
			if m != nil {
				weight, _ = strconv.Atoi(m[1])
			}
			if weight < 0 || weight > 100 {
				*issues = append(*issues, label+": an award criterion's weight is not a percentage")
				continue
			}
			item.Weight = &weight
		}
		out = append(out, item)
	}
	return out
}

// dateAt reads a date at the steps below n as its calendar day, at midnight UTC. The zone a date
// carries does not move the day.
func (r *reader) dateAt(n *node, issues *[]string, label string, steps ...xml.Name) *time.Time {
	day := firstNode(r.walk(n, steps...))
	if day == nil {
		return nil
	}
	date, err := time.Parse(zonedDate, day.textValue())
	if err != nil {
		date, err = time.Parse(isoDate, day.textValue())
	}
	if err != nil {
		*issues = append(*issues, label+": a date is not a date")
		return nil
	}
	t := time.Date(date.Year(), date.Month(), date.Day(), 0, 0, 0, 0, time.UTC)
	return &t
}
