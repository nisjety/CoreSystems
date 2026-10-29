package eforms

import (
	"strconv"
	"time"
)

// durationUnits are the units of the eForms duration-unit code list. A duration in any other unit is
// an issue: a length whose unit the parser does not know is not a length.
var durationUnits = map[string]bool{"DAY": true, "WEEK": true, "MONTH": true, "YEAR": true}

// periodUnlimited is the timeperiod code a notice gives for a contract with no end.
const periodUnlimited = "UNLIMITED"

// contractPeriod reads the contract period of a lot's project (cac:PlannedPeriod): start and end as
// calendar days, a duration with its unit, and whether the notice says it has no end. It is nil when
// the lot states no period. A value that cannot be read is an issue and is left out; the rest of the
// period is still read.
func (r *reader) contractPeriod(project *node, label string, issues *[]string) *ContractPeriod {
	period := firstNode(r.walk(project, cac("PlannedPeriod")))
	if period == nil {
		return nil
	}
	p := ContractPeriod{
		Start: r.dateAt(period, issues, label+" contract", cbc("StartDate")),
		End:   r.dateAt(period, issues, label+" contract", cbc("EndDate")),
	}
	if m := firstNode(r.walk(period, cbc("DurationMeasure"))); m != nil {
		value, unit := m.textValue(), m.attr("unitCode")
		n, err := strconv.Atoi(value)
		if !countValue.MatchString(value) || err != nil || n < 1 || !durationUnits[unit] {
			*issues = append(*issues, label+": a contract duration is not a whole number of days, weeks, months or years")
		} else {
			p.Duration = &Duration{Value: n, Unit: unit}
		}
	}
	if code := firstText(r.walk(period, cbc("DescriptionCode"))); code == periodUnlimited {
		p.Unlimited = true
	}
	return &p
}

// renewals reads the most renewals the buyer may make of a lot's contract (BT-58), or nil when the
// notice states none. A value that is not a whole number is an issue.
func (r *reader) renewals(project *node, label string, issues *[]string) *int {
	v := firstNode(r.walk(project, cac("ContractExtension"), cbc("MaximumNumberNumeric")))
	if v == nil {
		return nil
	}
	if !countValue.MatchString(v.textValue()) {
		*issues = append(*issues, label+": the number of renewals is not a whole number")
		return nil
	}
	n, _ := strconv.Atoi(v.textValue())
	return &n
}

// settledContracts reads the contracts of a result by their identifiers, with the day each was
// concluded (nil when the notice states none). The tenders a contract refers to are the same ones
// the lot results name, and are read there.
func (r *reader) settledContracts(result *node, issues *[]string) map[string]*time.Time {
	found := map[string]*time.Time{}
	for _, c := range r.walk(result, efac("SettledContract")) {
		id := firstText(r.walk(c, cbc("ID")))
		found[id] = r.dateAt(c, issues, "contract", cbc("IssueDate"))
		r.walk(c, efac("LotTender"), cbc("ID"))
	}
	return found
}
