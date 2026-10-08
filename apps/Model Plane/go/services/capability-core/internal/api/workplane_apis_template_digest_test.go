package api

import (
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"testing"
)

// The shared template-digest vector.
//
// Control commits to a digest of the schedule's task template that the BFF
// (Rust) computes and Capability Core (Go) recomputes, so both must hash the
// same bytes. The same template and the same expected digest are pinned in
// apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces/schedule_template.rs
// (Rust), in taskexec (workflow_dispatcher_template_digest_test.go) and in
// orchestrator-core (workflowreg_template_digest_test.go). Change it in all four
// places or none.
const scheduleDigestVectorTemplate = `{"title":"Ukentlig <rapport> & oppsummering","kind":"cron","description":"Sammenlign \"A\" & B: <b>fet</b> > C. Æ Ø Å æ ø å 日本語 \ud83d\ude00 linje\u2028to\u2029tre\ttab\nny \\ bak / frem \u0001\u0008\u000c\u007f slutt","priority":3,"enabled":true,"assignee":null,"x<y&z":"nøkkel"}`

const scheduleDigestVectorCanonicalHead = `{"assignee":null,"description":"Sammenlign \"A\" \u0026 B: \u003cb\u003efet\u003c/b\u003e \u003e C. Æ Ø Å æ ø å 日本語 😀 linje\u2028to\u2029tre\ttab\nny \\ bak / frem \u0001\b\f`

const scheduleDigestVectorCanonicalTail = ` slutt","enabled":true,"kind":"cron","priority":3,"title":"Ukentlig \u003crapport\u003e \u0026 oppsummering","x\u003cy\u0026z":"nøkkel"}`

// scheduleDigestVectorCanonical is json.Marshal of the template; the DEL
// character (U+007F) is spliced in as an escape so the literal stays readable.
const scheduleDigestVectorCanonical = scheduleDigestVectorCanonicalHead + "\x7f" + scheduleDigestVectorCanonicalTail

const scheduleDigestVectorDigest = "sha256:9a15f77fa517dc15cc4a0933b824a31449dbdad178b286dd74d58c573615cd61"

// TestCreateScheduleHashesTheSharedVectorToTheBFFDigest pins what the create
// handler computes for a template full of the characters Go escapes and
// serde_json does not. The handler decodes the body into cronScheduleRow,
// marshals TaskTemplate and hashes the bytes; the two source assertions keep
// this test's copy of those lines honest.
func TestCreateScheduleHashesTheSharedVectorToTheBFFDigest(t *testing.T) {
	body := `{"id":"cron-1","name":"Ukentlig","schedule_expr":"0 9 * * *","task_template":` + scheduleDigestVectorTemplate + `}`
	var s cronScheduleRow
	if err := json.NewDecoder(strings.NewReader(body)).Decode(&s); err != nil {
		t.Fatalf("decode create body: %v", err)
	}
	tplJSON, _ := json.Marshal(s.TaskTemplate)
	templateDigest := fmt.Sprintf("sha256:%x", sha256.Sum256(tplJSON))

	if string(tplJSON) != scheduleDigestVectorCanonical {
		t.Fatalf("canonical template bytes drifted\n got: %q\nwant: %q", tplJSON, scheduleDigestVectorCanonical)
	}
	if templateDigest != scheduleDigestVectorDigest {
		t.Fatalf("template digest = %s, want %s", templateDigest, scheduleDigestVectorDigest)
	}

	source, err := os.ReadFile("workplane_apis.go")
	if err != nil {
		t.Fatalf("read handler source: %v", err)
	}
	for _, line := range []string{
		`tplJSON, _ := json.Marshal(s.TaskTemplate)`,
		`templateDigest := fmt.Sprintf("sha256:%x", sha256.Sum256(tplJSON))`,
	} {
		if !strings.Contains(string(source), line) {
			t.Fatalf("the create handler no longer computes the digest as this test and the BFF assume: missing %q", line)
		}
	}
}

// TestGoMarshalEscapesExactlyTheCharactersTheBFFHelperEscapes pins the Go
// behaviour the Rust helper go_compatible_json_bytes mirrors: < > & and
// U+2028/U+2029 become \u escapes, \b and \f use their short forms (Go 1.22+),
// and nothing else that serde_json leaves alone is touched.
func TestGoMarshalEscapesExactlyTheCharactersTheBFFHelperEscapes(t *testing.T) {
	escaped, err := json.Marshal("<>&\u2028\u2029")
	if err != nil {
		t.Fatal(err)
	}
	if want := `"\u003c\u003e\u0026\u2028\u2029"`; string(escaped) != want {
		t.Fatalf("escaped = %s, want %s", escaped, want)
	}

	untouched := "'\"/\\é\x7f\u2027\u202a\U0001F600\x01\x08\x0c\n\r\t"
	got, err := json.Marshal(untouched)
	if err != nil {
		t.Fatal(err)
	}
	if want := "\"'\\\"/\\\\é\x7f\u2027\u202a\U0001F600\\u0001\\b\\f\\n\\r\\t\""; string(got) != want {
		t.Fatalf("marshal = %q, want %q", got, want)
	}

	keyed, err := json.Marshal(map[string]string{"a<b": "v"})
	if err != nil {
		t.Fatal(err)
	}
	if want := `{"a\u003cb":"v"}`; string(keyed) != want {
		t.Fatalf("map key escaping = %s, want %s", keyed, want)
	}
}
