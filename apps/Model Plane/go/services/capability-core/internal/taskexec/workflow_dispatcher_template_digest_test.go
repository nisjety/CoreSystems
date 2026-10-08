package taskexec

import (
	"encoding/json"
	"testing"
)

// The shared template-digest vector. See
// internal/api/workplane_apis_template_digest_test.go for why it is pinned in
// four places; change it in all four or none.
const dispatcherDigestVectorTemplate = `{"title":"Ukentlig <rapport> & oppsummering","kind":"cron","description":"Sammenlign \"A\" & B: <b>fet</b> > C. Æ Ø Å æ ø å 日本語 \ud83d\ude00 linje\u2028to\u2029tre\ttab\nny \\ bak / frem \u0001\u0008\u000c\u007f slutt","priority":3,"enabled":true,"assignee":null,"x<y&z":"nøkkel"}`

const dispatcherDigestVectorCanonicalHead = `{"assignee":null,"description":"Sammenlign \"A\" \u0026 B: \u003cb\u003efet\u003c/b\u003e \u003e C. Æ Ø Å æ ø å 日本語 😀 linje\u2028to\u2029tre\ttab\nny \\ bak / frem \u0001\b\f`

const dispatcherDigestVectorCanonicalTail = ` slutt","enabled":true,"kind":"cron","priority":3,"title":"Ukentlig \u003crapport\u003e \u0026 oppsummering","x\u003cy\u0026z":"nøkkel"}`

const dispatcherDigestVectorCanonical = dispatcherDigestVectorCanonicalHead + "\x7f" + dispatcherDigestVectorCanonicalTail

const dispatcherDigestVectorDigest = "sha256:9a15f77fa517dc15cc4a0933b824a31449dbdad178b286dd74d58c573615cd61"

// postgresRenderedVectorConfig is tasks.config_json for a task fired from a
// schedule whose template is the shared vector, exactly as Postgres 16 renders
// the JSONB the sweeper wrote (captured from postgres:16-alpine by
// TestScheduleTemplateDigestSurvivesJSONB, which asserts the live behaviour).
// JSONB has already decoded every escape and stores keys shortest first, so the
// text carries raw < > & characters, the real U+2028 and U+2029, a raw DEL, and
// only the escapes JSONB itself writes (\" \\ \t \n \b \f and \u00XX for other
// controls). The dispatcher never sees the JSON the browser sent, only this.
const postgresRenderedVectorConfigHead = `{"kind": "cron", "title": "Ukentlig <rapport> & oppsummering", "x<y&z": "nøkkel", "enabled": true, "assignee": null, "priority": 3, "description": "Sammenlign \"A\" & B: <b>fet</b> > C. Æ Ø Å æ ø å 日本語 😀 linje`

const postgresRenderedVectorConfigMiddle = `tre\ttab\nny \\ bak / frem \u0001\b\f`

const postgresRenderedVectorConfigTail = ` slutt", "schedule_fire_intent": {"OrgID": "org-1", "FireKey": "2026-10-01T07:22:43.830905Z", "SpaceRef": "space-1", "SubjectID": "user-1", "ScheduleID": "cron-0", "IdempotencyKey": "cron-0:2026-10-01T07:22:43.830905Z", "TemplateDigest": "sha256:9a15f77fa517dc15cc4a0933b824a31449dbdad178b286dd74d58c573615cd61"}}`

const postgresRenderedVectorConfig = postgresRenderedVectorConfigHead + "\u2028to\u2029" + postgresRenderedVectorConfigMiddle + "\x7f" + postgresRenderedVectorConfigTail

// TestCanonicalScheduledTemplateOfThePostgresRenderedVectorMatchesTheBFFDigest
// is the fire-time half of the parity: the digest the dispatcher recomputes from
// the stored task config must be the one the BFF asked Control to commit to at
// create. Go re-escapes < > & and U+2028/U+2029 when it re-marshals, which is
// why the digest Go committed at create is reproducible here at all.
func TestCanonicalScheduledTemplateOfThePostgresRenderedVectorMatchesTheBFFDigest(t *testing.T) {
	canonical, digest, err := canonicalScheduledTemplate(json.RawMessage(postgresRenderedVectorConfig))
	if err != nil {
		t.Fatalf("canonicalScheduledTemplate: %v", err)
	}
	if string(canonical) != dispatcherDigestVectorCanonical {
		t.Fatalf("canonical template bytes drifted\n got: %q\nwant: %q", canonical, dispatcherDigestVectorCanonical)
	}
	if digest != dispatcherDigestVectorDigest {
		t.Fatalf("template digest = %s, want %s", digest, dispatcherDigestVectorDigest)
	}
}

// TestCanonicalScheduledTemplateDropsOnlyTheFireIntent pins the rule the BFF
// reserves schedule_fire_intent for: the key is removed before hashing, so a
// template carrying its own copy hashes differently at fire than at create.
func TestCanonicalScheduledTemplateDropsOnlyTheFireIntent(t *testing.T) {
	_, withIntent, err := canonicalScheduledTemplate(json.RawMessage(`{"kind":"cron","schedule_fire_intent":{"a":1}}`))
	if err != nil {
		t.Fatal(err)
	}
	_, withoutIntent, err := canonicalScheduledTemplate(json.RawMessage(`{"kind":"cron"}`))
	if err != nil {
		t.Fatal(err)
	}
	if withIntent != withoutIntent {
		t.Fatalf("schedule_fire_intent changed the digest: %s != %s", withIntent, withoutIntent)
	}
	_, other, err := canonicalScheduledTemplate(json.RawMessage(`{"kind":"cron","other":{"a":1}}`))
	if err != nil {
		t.Fatal(err)
	}
	if other == withoutIntent {
		t.Fatal("only schedule_fire_intent may be dropped before hashing")
	}
}
