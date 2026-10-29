package eforms

import (
	"bytes"
	"errors"
	"fmt"
	"strings"
	"testing"
)

// notice builds a minimal eForms document with the shape of a real
// ContractNotice (same element names, namespaces and nesting) but entirely
// invented values.
func notice(customization string) string {
	return fmt.Sprintf(`<?xml version="1.0" encoding="UTF-8"?>
<ContractNotice xmlns="urn:oasis:names:specification:ubl:schema:xsd:ContractNotice-2"
 xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2"
 xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
 xmlns:ext="urn:oasis:names:specification:ubl:schema:xsd:CommonExtensionComponents-2"
 xmlns:efac="http://data.europa.eu/p27/eforms-ubl-extension-aggregate-components/1"
 xmlns:efext="http://data.europa.eu/p27/eforms-ubl-extensions/1">
  <ext:UBLExtensions><ext:UBLExtension><ext:ExtensionContent><efext:EformsExtension>
    <efac:NoticeSubType><cbc:SubTypeCode listName="notice-subtype">16</cbc:SubTypeCode></efac:NoticeSubType>
  </efext:EformsExtension></ext:ExtensionContent></ext:UBLExtension></ext:UBLExtensions>
  <cbc:UBLVersionID>2.3</cbc:UBLVersionID>
  <cbc:CustomizationID>%s</cbc:CustomizationID>
  <cbc:ID schemeName="notice-id">00000000-0000-4000-8000-000000000001</cbc:ID>
  <cbc:ContractFolderID>00000000-0000-4000-8000-0000000000aa</cbc:ContractFolderID>
  <cbc:VersionID>01</cbc:VersionID>
  <cbc:NoticeTypeCode listName="competition">cn-standard</cbc:NoticeTypeCode>
  <cac:ProcurementProject>
    <cbc:ID>NESTED-ID-MUST-NOT-BE-READ</cbc:ID>
    <cbc:VersionID>99</cbc:VersionID>
  </cac:ProcurementProject>
</ContractNotice>`, customization)
}

const euCustomization = "eforms-sdk-1.14#extended#urn:fdc:anskaffelser.no:2023:eforms:eu"

// inject puts inner just inside the closing root tag.
func inject(doc, inner string) string {
	return strings.Replace(doc, "</ContractNotice>", inner+"</ContractNotice>", 1)
}

func TestSniffReadsTheHeaderOfAContractNotice(t *testing.T) {
	t.Parallel()

	h, err := Sniff([]byte(notice(euCustomization)))
	if err != nil {
		t.Fatalf("Sniff: %v", err)
	}
	want := Header{
		Root:            "ContractNotice",
		CustomizationID: euCustomization,
		SDKVersion:      "1.14",
		Flavour:         FlavourEU,
		NoticeID:        "00000000-0000-4000-8000-000000000001",
		VersionID:       "01",
		ProcedureID:     "00000000-0000-4000-8000-0000000000aa",
		NoticeTypeCode:  "cn-standard",
		SubTypeCode:     "16",
	}
	if h != want {
		t.Fatalf("header = %+v\nwant     %+v", h, want)
	}
}

// Nested elements reuse the names ID and VersionID; only the root's own
// children identify the notice.
func TestSniffDoesNotConfuseNestedIdentifiers(t *testing.T) {
	t.Parallel()

	h, err := Sniff([]byte(notice(euCustomization)))
	if err != nil {
		t.Fatalf("Sniff: %v", err)
	}
	if strings.Contains(h.NoticeID, "NESTED") || h.VersionID == "99" {
		t.Fatalf("a nested identifier leaked into the header: %+v", h)
	}
}

func TestSniffRecognisesTheNorwegianFlavours(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name          string
		customization string
		wantSDK       string
		wantFlavour   Flavour
	}{
		{"above threshold", "eforms-sdk-1.14#extended#urn:fdc:anskaffelser.no:2023:eforms:eu", "1.14", FlavourEU},
		{"national", "eforms-sdk-1.8#extended#urn:fdc:anskaffelser.no:2023:eforms:national", "1.8", FlavourNational},
		{"new national", "eforms-sdk-1.12#extended#urn:fdc:anskaffelser.no:2026:eforms:national-e", "1.12", FlavourNationalE},
		{"patch version", "eforms-sdk-1.15.1#extended#urn:fdc:anskaffelser.no:2023:eforms:eu", "1.15.1", FlavourEU},
		{"plain EU eForms, no Norwegian extension", "eforms-sdk-1.13", "1.13", ""},
		{"a flavour this code has never heard of", "eforms-sdk-1.14#extended#urn:fdc:anskaffelser.no:2031:eforms:future", "1.14", Flavour("future")},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			h, err := Sniff([]byte(notice(tc.customization)))
			if err != nil {
				t.Fatalf("Sniff: %v", err)
			}
			if h.SDKVersion != tc.wantSDK || h.Flavour != tc.wantFlavour {
				t.Fatalf("SDK/flavour = %q/%q, want %q/%q", h.SDKVersion, h.Flavour, tc.wantSDK, tc.wantFlavour)
			}
		})
	}
}

func TestFlavourKnown(t *testing.T) {
	t.Parallel()

	for _, f := range []Flavour{FlavourEU, FlavourNational, FlavourNationalE} {
		if !f.Known() {
			t.Errorf("%q should be known", f)
		}
	}
	for _, f := range []Flavour{"", "future", "EU"} {
		if f.Known() {
			t.Errorf("%q should not be known: an unknown flavour must make the parser fail closed", f)
		}
	}
}

func TestSniffRejectsDocumentsThatAreNotEForms(t *testing.T) {
	t.Parallel()

	tests := map[string]string{
		"empty":                          "",
		"whitespace":                     "   \n",
		"html":                           "<html><body>maintenance</body></html>",
		"plain text":                     "Access denied",
		"json":                           `{"hits":[]}`,
		"xml without a customization id": `<ContractNotice xmlns="urn:oasis:names:specification:ubl:schema:xsd:ContractNotice-2"><ID>1</ID></ContractNotice>`,
		"truncated mid element":          `<ContractNotice xmlns="urn:x"><CustomizationID>eforms-sdk-1.14</Custo`,
		"not even well formed":           `<a><b></a>`,
	}
	for name, doc := range tests {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			if _, err := Sniff([]byte(doc)); !errors.Is(err, ErrNotEForms) {
				t.Fatalf("error = %v, want ErrNotEForms", err)
			}
		})
	}
}

// The checks below exist because Sniff routes notices: a document that merely
// looks like one must not be waved through to a parser that then disagrees.
func TestSniffRejectsStructurallySuspiciousDocuments(t *testing.T) {
	t.Parallel()

	base := notice(euCustomization)
	manyAttributes := make([]string, 0, 200)
	for i := range 200 {
		manyAttributes = append(manyAttributes, fmt.Sprintf(`a%d=""`, i))
	}

	tests := []struct {
		name string
		doc  string
	}{
		{"a second top-level element", base + `<Other xmlns="urn:x"/>`},
		{"text after the root", base + "trailing text"},
		{"text before the root", "leading text" + base},
		{"a duplicated CustomizationID", inject(base, `<cbc:CustomizationID>eforms-sdk-1.8</cbc:CustomizationID>`)},
		{"a duplicated notice id", inject(base, `<cbc:ID schemeName="notice-id">other</cbc:ID>`)},
		{"a duplicated subtype code", strings.Replace(base, "</efac:NoticeSubType>", `<cbc:SubTypeCode>99</cbc:SubTypeCode></efac:NoticeSubType>`, 1)},
		{"a root that is not a UBL document", strings.Replace(base, `xmlns="urn:oasis:names:specification:ubl:schema:xsd:ContractNotice-2"`, `xmlns="urn:x"`, 1)},
		{"header elements in the wrong namespace", strings.Replace(base, "urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2", "urn:evil", 1)},
		{"a CustomizationID that names no SDK version", notice("something-else")},
		{"a header value that is absurdly long", notice(strings.Repeat("a", 5000))},
		{"an attribute flood on the root", strings.Replace(base, "<ContractNotice ", "<ContractNotice "+strings.Join(manyAttributes, " ")+" ", 1)},
		{"nesting far deeper than any notice", inject(base, strings.Repeat("<x>", 200)+strings.Repeat("</x>", 200))},
		{"millions of elements", inject(base, strings.Repeat("<x/>", 1_300_000))},
		{"namespace declarations piling up across elements", inject(base, strings.Repeat(`<x xmlns:p="urn:x"/>`, 70))},
		// A valid notice whose single tag is longer than any real one. Without the run
		// check this document is accepted, so the test pins that check on its own.
		{"a single tag over a megabyte", strings.Replace(base, "<ContractNotice ", `<ContractNotice x="`+strings.Repeat("a", 1<<20+1)+`" `, 1)},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			if _, err := Sniff([]byte(tc.doc)); !errors.Is(err, ErrNotEForms) {
				t.Fatalf("error = %v, want ErrNotEForms", err)
			}
		})
	}
}

func TestSniffRefusesADocumentOverTheSizeLimit(t *testing.T) {
	t.Parallel()

	_, err := Sniff(bytes.Repeat([]byte(" "), MaxNoticeBytes+1))
	if !errors.Is(err, ErrTooLarge) {
		t.Fatalf("error = %v, want ErrTooLarge", err)
	}
}

// An error that quotes the document would carry attacker-chosen bytes (bidi
// overrides, C1 controls) to the first caller that logs it.
func TestSniffErrorsCarryNoDocumentBytes(t *testing.T) {
	t.Parallel()

	for name, doc := range map[string]string{
		"hostile characters in a name": "<a‮\u009b/>",
		"two huge mismatched names":    "<" + strings.Repeat("a", 200000) + "></" + strings.Repeat("b", 200000) + ">",
		"a hostile entity":             "<a>&‮evil;</a>",
	} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			_, err := Sniff([]byte(doc))
			if !errors.Is(err, ErrNotEForms) {
				t.Fatalf("error = %v, want ErrNotEForms", err)
			}
			if len(err.Error()) > 200 {
				t.Errorf("the error is %d bytes long; it should be a short constant description", len(err.Error()))
			}
			for _, r := range err.Error() {
				if r == '‮' || r == '\u009b' || r == 'a' && strings.Count(err.Error(), "a") > 60 {
					t.Errorf("the error quotes the document: %q", err.Error()[:min(len(err.Error()), 120)])
					break
				}
			}
		})
	}
}

func TestSniffSaysWhichLineIsMalformed(t *testing.T) {
	t.Parallel()

	// A UBL root, so the failure is the mismatched tag on line 2 and not the root check.
	doc := "<ContractNotice xmlns=\"urn:oasis:names:specification:ubl:schema:xsd:ContractNotice-2\">\n<b></ContractNotice>"
	_, err := Sniff([]byte(doc))
	if err == nil || !strings.Contains(err.Error(), "line 2") {
		t.Fatalf("error = %v, want one naming line 2", err)
	}
}

func TestSniffToleratesAByteOrderMark(t *testing.T) {
	t.Parallel()

	doc := append([]byte{0xEF, 0xBB, 0xBF}, []byte(notice(euCustomization))...)
	if _, err := Sniff(doc); err != nil {
		t.Fatalf("a UTF-8 byte order mark must not break detection: %v", err)
	}
}

// A hostile document must not be able to make the sniffer read local files or
// expand entities.
func TestSniffDoesNotResolveExternalEntities(t *testing.T) {
	t.Parallel()

	doc := `<?xml version="1.0"?>
<!DOCTYPE ContractNotice [<!ENTITY secret SYSTEM "file:///etc/hosts">]>
<ContractNotice xmlns="urn:oasis:names:specification:ubl:schema:xsd:ContractNotice-2"
 xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
  <cbc:CustomizationID>eforms-sdk-1.14</cbc:CustomizationID><cbc:ID>&secret;</cbc:ID>
</ContractNotice>`
	h, err := Sniff([]byte(doc))
	if err == nil && strings.Contains(h.NoticeID, "localhost") {
		t.Fatalf("an external entity was resolved: %+v", h)
	}
	if !errors.Is(err, ErrNotEForms) {
		t.Fatalf("error = %v, want ErrNotEForms for an undefined entity", err)
	}
}

func TestSniffRefusesAbsurdNestingBeforeItCostsMemory(t *testing.T) {
	t.Parallel()

	// A valid header first, so the nesting check is what stops it, not a missing field.
	doc := inject(notice(euCustomization), strings.Repeat("<x>", 20000)+strings.Repeat("</x>", 20000))
	if _, err := Sniff([]byte(doc)); !errors.Is(err, ErrNotEForms) {
		t.Fatalf("error = %v, want ErrNotEForms for a document nested 20000 deep", err)
	}
}
