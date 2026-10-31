package main

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

func accepting(ids ...string) *fakeNotices {
	return &fakeNotices{search: func(doffin.SearchParams) (doffin.SearchResponse, error) { return response(ids...), nil }}
}

func rejecting(status int) *fakeNotices {
	return &fakeNotices{search: func(doffin.SearchParams) (doffin.SearchResponse, error) {
		return doffin.SearchResponse{}, &doffin.HTTPError{Operation: "search", Status: status, Message: "Access denied"}
	}}
}

func TestProbeUsesTheSecondaryKeyWhenThePrimaryIsRejected(t *testing.T) {
	t.Parallel()

	primary, secondary := rejecting(401), accepting("2026-100001")
	var out bytes.Buffer
	p := &probe{out: &out, keys: []keyedClient{{"primary", primary}, {"secondary", secondary}}}

	if err := p.run(context.Background(), options{}); err != nil {
		t.Fatalf("run: %v", err)
	}
	text := out.String()
	for _, want := range []string{"primary", "REJECTED", "secondary", "accepted", "continuing with the secondary"} {
		if !strings.Contains(text, want) {
			t.Errorf("output should mention %q:\n%s", want, text)
		}
	}
	if p.client != notices(secondary) {
		t.Error("the rest of the probe must run on the key that works")
	}
}

func TestProbeKeepsThePrimaryWhenBothKeysWork(t *testing.T) {
	t.Parallel()

	primary, secondary := accepting("2026-100001"), accepting("2026-100001")
	var out bytes.Buffer
	p := &probe{out: &out, keys: []keyedClient{{"primary", primary}, {"secondary", secondary}}}

	if err := p.run(context.Background(), options{}); err != nil {
		t.Fatalf("run: %v", err)
	}
	if p.client != notices(primary) {
		t.Error("the primary should stay in use when it works")
	}
	if strings.Contains(out.String(), "REJECTED") || strings.Contains(out.String(), "continuing with the secondary") {
		t.Errorf("nothing was wrong:\n%s", out.String())
	}
}

func TestProbeFailsWhenNeitherKeyIsAccepted(t *testing.T) {
	t.Parallel()

	p := &probe{out: &bytes.Buffer{}, keys: []keyedClient{{"primary", rejecting(401)}, {"secondary", rejecting(403)}}}
	err := p.run(context.Background(), options{})
	if err == nil || !strings.Contains(err.Error(), "neither key") {
		t.Fatalf("error = %v, want one saying neither key was accepted", err)
	}
	if !strings.Contains(err.Error(), "Public API") {
		t.Errorf("the error should point at the usual cause (wrong API or portal): %v", err)
	}
}

func TestProbeReportsASecondaryKeyIdenticalToThePrimary(t *testing.T) {
	t.Parallel()

	// run never adds an identical secondary to keys, so only the primary is checked.
	var out bytes.Buffer
	p := &probe{out: &out, sameKey: true, keys: []keyedClient{{"primary", accepting("2026-100001")}}}

	if err := p.run(context.Background(), options{}); err != nil {
		t.Fatalf("run: %v", err)
	}
	if !strings.Contains(out.String(), "identical") {
		t.Errorf("the output should say the secondary adds no rotation safety:\n%s", out.String())
	}
}

// A 429 says nothing about the key; blaming it would send the owner chasing the
// wrong problem.
func TestProbeStopsAtARateLimitWithoutBlamingTheKey(t *testing.T) {
	t.Parallel()

	limited := &fakeNotices{search: func(doffin.SearchParams) (doffin.SearchResponse, error) {
		return doffin.SearchResponse{}, &doffin.RateLimitedError{Operation: "search", RetryAfter: 90 * time.Second}
	}}
	var out bytes.Buffer
	p := &probe{out: &out, keys: []keyedClient{{"primary", limited}}}

	err := p.run(context.Background(), options{})
	var rateErr *doffin.RateLimitedError
	if !errors.As(err, &rateErr) {
		t.Fatalf("error = %v, want the rate-limit error", err)
	}
	if strings.Contains(out.String(), "REJECTED") {
		t.Errorf("a rate limit is not a rejected key:\n%s", out.String())
	}
}

func TestExtensionIsChosenFromTheBodyBecauseTheHeaderCannotBeTrusted(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name        string
		contentType string
		body        string
		want        string
	}{
		{"doffin sends xml as octet-stream", "application/octet-stream", `<?xml version="1.0"?><ContractNotice/>`, ".xml"},
		{"xml with a byte order mark", "application/octet-stream", "\xef\xbb\xbf<?xml version=\"1.0\"?><a/>", ".xml"},
		{"json", "application/octet-stream", `  {"a":1}`, ".json"},
		{"pdf", "application/octet-stream", "%PDF-1.7 ...", ".pdf"},
		{"zip", "application/octet-stream", "PK\x03\x04...", ".zip"},
		{"unknown binary falls back to the header", "application/xml", "\x00\x01\x02", ".xml"},
		{"nothing to go on", "", "\x00\x01\x02", ".bin"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			if got := extensionFor(tc.contentType, []byte(tc.body)); got != tc.want {
				t.Fatalf("extensionFor = %q, want %q", got, tc.want)
			}
		})
	}
}

// eformsDoc builds a minimal document with the shape of a real notice and a
// platform link, for the sampling tests.
func eformsDoc(customization, host string) string {
	return fmt.Sprintf(`<?xml version="1.0" encoding="UTF-8"?>
<ContractNotice xmlns="urn:oasis:names:specification:ubl:schema:xsd:ContractNotice-2"
 xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2"
 xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2">
  <cbc:CustomizationID>%s</cbc:CustomizationID>
  <cbc:ID schemeName="notice-id">00000000-0000-4000-8000-000000000001</cbc:ID>
  <cbc:ContractFolderID>00000000-0000-4000-8000-0000000000aa</cbc:ContractFolderID>
  <cbc:VersionID>01</cbc:VersionID>
  <cbc:NoticeTypeCode listName="competition">cn-standard</cbc:NoticeTypeCode>
  <cac:ProcurementProjectLot><cac:TenderingTerms><cac:TenderRecipientParty>
    <cbc:EndpointID>https://%s/123.aspx</cbc:EndpointID>
  </cac:TenderRecipientParty></cac:TenderingTerms></cac:ProcurementProjectLot>
</ContractNotice>`, customization, host)
}

func TestSampleDownloadsSniffsAndSummarises(t *testing.T) {
	t.Parallel()

	var typesAsked []string
	client := &fakeNotices{
		search: func(p doffin.SearchParams) (doffin.SearchResponse, error) {
			typ := "ANY"
			if len(p.Types) > 0 {
				typ = p.Types[0]
			}
			typesAsked = append(typesAsked, typ)
			n := max(p.NumHitsPerPage, 1)
			ids := make([]string, 0, n)
			for i := range n {
				ids = append(ids, fmt.Sprintf("2026-%06d", len(typesAsked)*100+i))
			}
			if p.SortBy == "PUBLICATION_DATE_ASC" {
				ids = []string{"2017-011757"}
			}
			return response(ids...), nil
		},
		download: func(id string) (doffin.Notice, error) {
			switch {
			case strings.HasPrefix(id, "2017-"):
				return doffin.Notice{ID: id, ContentType: "text/html", Body: []byte("<html>legacy</html>"), SHA256: "x"}, nil
			case strings.HasSuffix(id, "1"):
				return doffin.Notice{ID: id, ContentType: "application/octet-stream", Body: []byte(eformsDoc("eforms-sdk-1.8#extended#urn:fdc:anskaffelser.no:2023:eforms:national", "kgv.example.no")), SHA256: "y"}, nil
			default:
				return doffin.Notice{ID: id, ContentType: "application/octet-stream", Body: []byte(eformsDoc("eforms-sdk-1.14#extended#urn:fdc:anskaffelser.no:2023:eforms:eu", "permalink.mercell.com")), SHA256: "z"}, nil
			}
		},
	}
	dir := t.TempDir()
	var out bytes.Buffer
	p := &probe{out: &out, client: client, saveDir: dir}

	if err := p.run(context.Background(), options{samples: 3}); err != nil {
		t.Fatalf("run: %v", err)
	}
	text := out.String()

	for _, want := range []string{"flavours:", "eu=", "national=", "roots:", "ContractNotice=", "permalink.mercell.com=", "not an eForms notice"} {
		if !strings.Contains(text, want) {
			t.Errorf("the sample summary should contain %q:\n%s", want, text)
		}
	}
	for _, typ := range []string{"ANNOUNCEMENT_OF_COMPETITION", "ADVISORY_NOTICE", "ANNOUNCEMENT_OF_CONCLUSION_OF_CONTRACT", "CANCELLED_OR_MISSING_CONCLUSION_OF_CONTRACT"} {
		found := false
		for _, asked := range typesAsked {
			found = found || asked == typ
		}
		if !found {
			t.Errorf("the sample never asked for type %s (asked: %v)", typ, typesAsked)
		}
	}
	xmlFiles, _ := filepath.Glob(filepath.Join(dir, "sample-*.xml"))
	if len(xmlFiles) == 0 {
		t.Error("downloaded notices should be saved with an .xml extension")
	}
	assertTerminalSafe(t, "output", text)
	if info, err := os.Stat(dir); err != nil || !info.IsDir() {
		t.Fatalf("save dir: %v", err)
	}
}

func TestSampleIgnoresPlatformHostsThatAreJustNamespaces(t *testing.T) {
	t.Parallel()

	hosts := urlHosts([]byte(`<a xmlns="http://data.europa.eu/p27/x" xmlns:b="http://www.w3.org/2001/XMLSchema">` +
		`<u>https://permalink.mercell.com/1.aspx</u><u>http://www.doffin.no/x</u><u>https://permalink.mercell.com/2.aspx</u></a>`))
	got := strings.Join(hosts, ",")
	if strings.Contains(got, "data.europa.eu") || strings.Contains(got, "w3.org") {
		t.Errorf("namespace hosts must be ignored: %s", got)
	}
	if !strings.Contains(got, "permalink.mercell.com") || !strings.Contains(got, "www.doffin.no") {
		t.Errorf("real hosts missing: %s", got)
	}
	if strings.Count(got, "permalink.mercell.com") != 1 {
		t.Errorf("hosts should be unique per notice: %s", got)
	}
}
