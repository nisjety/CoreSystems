package eforms

import (
	"regexp"
	"time"
)

// The shapes of a reference to a changed notice and of the other short values of a change section.
// A value of another shape is an issue and is left out: the parser does not guess what a reference
// means.
var (
	replacedNoticeForm = regexp.MustCompile(`^([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})-(\d{1,3})$`)
	publicationNumber  = regexp.MustCompile(`^\d{1,8}-\d{4}$`)
	reasonCodeForm     = regexp.MustCompile(`^[a-z0-9][a-z0-9-]{0,39}$`)
	sectionForm        = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$`)
)

// maxChangeSections bounds how many changed sections a notice lists.
const maxChangeSections = 100

// issuedAt reads the issue date and issue time (BT-803) of a notice. The calendar day is the one
// written; the hour comes with its zone. A missing or unreadable date or time gives no instant and an
// issue: the date alone is not one, and a zone is not chosen for the notice.
func (r *reader) issuedAt(root *node, issues *[]string) *time.Time {
	date := firstNode(r.walk(root, cbc("IssueDate")))
	clock := firstNode(r.walk(root, cbc("IssueTime")))
	if date == nil && clock == nil {
		return nil
	}
	if date == nil || clock == nil {
		*issues = append(*issues, "notice: the issue date and time are not both stated")
		return nil
	}
	day, err := time.Parse(zonedDate, date.textValue())
	if err != nil {
		day, err = time.Parse(isoDate, date.textValue())
	}
	if err != nil {
		*issues = append(*issues, "notice: the issue date is not a date")
		return nil
	}
	at, err := time.Parse(zonedClock, clock.textValue())
	if err != nil {
		if _, bare := time.Parse(bareClock, clock.textValue()); bare == nil {
			*issues = append(*issues, "notice: the issue time has no zone")
		} else {
			*issues = append(*issues, "notice: the issue time is not a time")
		}
		return nil
	}
	t := time.Date(day.Year(), day.Month(), day.Day(), at.Hour(), at.Minute(), at.Second(), 0, at.Location())
	return &t
}

// changes reads the change section of a notice (efac:Changes in the extension block). It is nil when
// the notice has none. Each value that cannot be read is an issue and is left out; the rest is read.
func (r *reader) changes(root *node, issues *[]string) *ChangeInfo {
	section := firstNode(r.walk(root, ext("UBLExtensions"), ext("UBLExtension"), ext("ExtensionContent"),
		efext("EformsExtension"), efac("Changes")))
	if section == nil {
		return nil
	}
	c := ChangeInfo{}
	if ref := firstText(r.walk(section, efbc("ChangedNoticeIdentifier"))); ref != "" {
		if replacedNoticeForm.MatchString(ref) || publicationNumber.MatchString(ref) {
			c.Replaces = ref
		} else {
			*issues = append(*issues, "change: the reference to the changed notice is not a notice identifier or a publication number")
		}
	}
	reason := firstNode(r.walk(section, efac("ChangeReason")))
	if reason != nil {
		if code := firstText(r.walk(reason, cbc("ReasonCode"))); code != "" {
			if reasonCodeForm.MatchString(code) {
				c.ReasonCode = code
			} else {
				*issues = append(*issues, "change: the reason code is not shaped like a code")
			}
		}
		c.Reason = preferNorwegian(r.walk(reason, efbc("ReasonDescription")))
	}
	for _, id := range r.walk(section, efac("Change"), efac("ChangedSection"), efbc("ChangedSectionIdentifier")) {
		v := id.textValue()
		if !sectionForm.MatchString(v) {
			*issues = append(*issues, "change: a changed section is not shaped like an identifier")
			continue
		}
		if len(c.Sections) < maxChangeSections && !contains(c.Sections, v) {
			c.Sections = append(c.Sections, v)
		}
	}
	return &c
}

func contains(list []string, s string) bool {
	for _, v := range list {
		if v == s {
			return true
		}
	}
	return false
}
