package eforms

import (
	"slices"
	"strings"
	"testing"
	"time"
)

const efbcNamespace = `xmlns:efbc="http://data.europa.eu/p27/eforms-ubl-extension-basic-components/1" xmlns:efext=`

// changed returns the synthetic contract notice laid out as a change notice is: the issue date and
// time of the notice, and a Changes section in the extension block, in the shape the captured change
// notices have (efac:Changes with efbc:ChangedNoticeIdentifier, efac:Change/efac:ChangedSection,
// efac:ChangeReason with a cbc:ReasonCode and efbc:ReasonDescription in each language).
func changed(changes string, pairs ...string) []byte {
	base := sample(
		` xmlns:efext=`, ` `+efbcNamespace,
		`<efac:NoticeSubType>`, changes+`<efac:NoticeSubType>`,
		`<cbc:NoticeTypeCode>cn-standard</cbc:NoticeTypeCode>`,
		`<cbc:NoticeTypeCode>cn-standard</cbc:NoticeTypeCode><cbc:IssueDate>2026-10-01+02:00</cbc:IssueDate><cbc:IssueTime>12:30:00+02:00</cbc:IssueTime>`,
	)
	if len(pairs) == 0 {
		return base
	}
	// A second pass: a replacement in a test aims at the text the first pass has just put in.
	return []byte(strings.NewReplacer(pairs...).Replace(string(base)))
}

const (
	oldNotice = "0cdb6ab9-aaaa-4bbb-8ccc-0123456789ab"

	changesSection = `<efac:Changes>
      <efbc:ChangedNoticeIdentifier>` + oldNotice + `-01</efbc:ChangedNoticeIdentifier>
      <efac:Change><efac:ChangedSection><efbc:ChangedSectionIdentifier>LOT-0001</efbc:ChangedSectionIdentifier></efac:ChangedSection></efac:Change>
      <efac:ChangeReason>
        <cbc:ReasonCode listName="change-corrig-justification">update-add</cbc:ReasonCode>
        <efbc:ReasonDescription languageID="ENG">The deadline is postponed.</efbc:ReasonDescription>
        <efbc:ReasonDescription languageID="NOR">Fristen er utsatt.</efbc:ReasonDescription>
      </efac:ChangeReason>
    </efac:Changes>`
)

// The issue date and time of a notice (BT-803) say when it was published, and order its versions. The
// day is the one written; the hour comes with its own zone, as for deadlines.
func TestParseReadsWhenANoticeWasIssued(t *testing.T) {
	t.Parallel()

	n, err := Parse(changed(""))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if want := time.Date(2026, time.October, 1, 10, 30, 0, 0, time.UTC); n.IssuedAt == nil || !n.IssuedAt.Equal(want) {
		t.Errorf("issued at = %v, want %v (12:30 at +02:00)", n.IssuedAt, want)
	}
	for _, issue := range n.Issues {
		t.Errorf("issue for a well-formed notice: %s", issue)
	}
}

func TestParseReadsAnIssueTimeInZuluWithFractionalSeconds(t *testing.T) {
	t.Parallel()

	n, err := Parse(changed("", "<cbc:IssueTime>12:30:00+02:00</cbc:IssueTime>", "<cbc:IssueTime>10:30:00.000Z</cbc:IssueTime>"))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if want := time.Date(2026, time.October, 1, 10, 30, 0, 0, time.UTC); n.IssuedAt == nil || !n.IssuedAt.Equal(want) {
		t.Errorf("issued at = %v, want %v", n.IssuedAt, want)
	}
}

// A notice that gives no usable time is not given one: the field is left empty and an issue says so,
// with no value in it. The date alone is not an instant.
func TestParseDoesNotInventAnIssueTime(t *testing.T) {
	t.Parallel()

	for name, doc := range map[string][]byte{
		"no time":        changed("", "<cbc:IssueTime>12:30:00+02:00</cbc:IssueTime>", ""),
		"no zone":        changed("", "<cbc:IssueTime>12:30:00+02:00</cbc:IssueTime>", "<cbc:IssueTime>12:30:00</cbc:IssueTime>"),
		"no date":        changed("", "<cbc:IssueDate>2026-10-01+02:00</cbc:IssueDate>", ""),
		"not a date":     changed("", "<cbc:IssueDate>2026-10-01+02:00</cbc:IssueDate>", "<cbc:IssueDate>yesterday</cbc:IssueDate>"),
		"not a time":     changed("", "<cbc:IssueTime>12:30:00+02:00</cbc:IssueTime>", "<cbc:IssueTime>noon</cbc:IssueTime>"),
		"nothing at all": sample(),
	} {
		n, err := Parse(doc)
		if err != nil {
			t.Fatalf("%s: Parse: %v", name, err)
		}
		if n.IssuedAt != nil {
			t.Errorf("%s: issued at = %v, want none", name, n.IssuedAt)
		}
		if name != "nothing at all" && len(n.Issues) == 0 {
			t.Errorf("%s: no issue recorded", name)
		}
		for _, issue := range n.Issues {
			for _, value := range []string{"yesterday", "noon"} {
				if strings.Contains(issue, value) {
					t.Errorf("%s: the issue quotes the value: %q", name, issue)
				}
			}
		}
	}
}

// A change notice states what it changes, why, in the buyer's words, and which sections.
func TestParseReadsTheChangesOfAChangeNotice(t *testing.T) {
	t.Parallel()

	n, err := Parse(changed(changesSection))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	c := n.Change
	if c == nil {
		t.Fatal("no change section read")
	}
	if c.Replaces != oldNotice+"-01" || c.ReasonCode != "update-add" || c.Reason != "Fristen er utsatt." || !slices.Equal(c.Sections, []string{"LOT-0001"}) {
		t.Errorf("change = %+v, want the old notice, update-add, the Norwegian text and LOT-0001", c)
	}
	if id, version, ok := c.ReplacedNotice(); !ok || id != oldNotice || version != "01" {
		t.Errorf("replaced notice = %q %q %v", id, version, ok)
	}
	for _, path := range n.Unmapped {
		if strings.Contains(path, "Changes") {
			t.Errorf("the change section is reported as unmapped: %q", path)
		}
	}
	for _, issue := range n.Issues {
		t.Errorf("issue for a well-formed change: %s", issue)
	}
}

// The reference to the changed notice may also be the TED publication number. It is kept as written,
// and it is not a notice identifier: nothing in a stored body can be matched to it.
func TestParseKeepsATEDPublicationNumberAsTheReference(t *testing.T) {
	t.Parallel()

	n, err := Parse(changed(changesSection, oldNotice+"-01", "123456-2026"))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	c := n.Change
	if c == nil || c.Replaces != "123456-2026" {
		t.Fatalf("change = %+v", c)
	}
	if _, _, ok := c.ReplacedNotice(); ok {
		t.Error("a publication number was read as a notice identifier")
	}
}

// A notice with no change section has no change, and the Norwegian text is preferred only when given.
func TestParseWithoutAChangeSectionHasNoChange(t *testing.T) {
	t.Parallel()

	n, err := Parse(changed(""))
	if err != nil || n.Change != nil {
		t.Errorf("change = %+v, err %v", n.Change, err)
	}
	n, err = Parse(changed(changesSection, `<efbc:ReasonDescription languageID="NOR">Fristen er utsatt.</efbc:ReasonDescription>`, ""))
	if err != nil || n.Change == nil || n.Change.Reason != "The deadline is postponed." {
		t.Errorf("with only the English text, reason = %+v (err %v)", n.Change, err)
	}
}

// A value of the section that cannot be read is an issue and is left out; the rest is still read. No
// issue quotes the value.
func TestParseDoesNotGuessWhatAChangeSaysItChanges(t *testing.T) {
	t.Parallel()

	for name, c := range map[string]struct {
		pairs []string
		check func(*ChangeInfo) bool
	}{
		"a reference of an unknown shape":   {[]string{oldNotice + "-01", "see the old notice"}, func(c *ChangeInfo) bool { return c.Replaces == "" && c.ReasonCode == "update-add" }},
		"a reason code of an unknown shape": {[]string{">update-add<", ">Update Add!<"}, func(c *ChangeInfo) bool { return c.ReasonCode == "" && c.Replaces != "" }},
		"a section of an unknown shape":     {[]string{">LOT-0001<", ">the first lot<"}, func(c *ChangeInfo) bool { return len(c.Sections) == 0 && c.Replaces != "" }},
	} {
		n, err := Parse(changed(changesSection, c.pairs...))
		if err != nil || n.Change == nil {
			t.Fatalf("%s: err %v change %+v", name, err, n.Change)
		}
		if !c.check(n.Change) {
			t.Errorf("%s: read as %+v", name, n.Change)
		}
		if len(n.Issues) == 0 {
			t.Errorf("%s: no issue recorded", name)
		}
		for _, issue := range n.Issues {
			for _, value := range []string{"see the old notice", "Update Add", "the first lot"} {
				if strings.Contains(issue, value) {
					t.Errorf("%s: the issue quotes the value: %q", name, issue)
				}
			}
		}
	}
}
