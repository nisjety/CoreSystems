package orchestration

import (
	"crypto/sha256"
	"fmt"
	"strings"
	"testing"
)

// The shared template-digest vector. See capability-core's
// internal/api/workplane_apis_template_digest_test.go for why it is pinned in
// four places; change it in all four or none.
const orchestratorDigestVectorCanonicalHead = `{"assignee":null,"description":"Sammenlign \"A\" \u0026 B: \u003cb\u003efet\u003c/b\u003e \u003e C. Æ Ø Å æ ø å 日本語 😀 linje\u2028to\u2029tre\ttab\nny \\ bak / frem \u0001\b\f`

const orchestratorDigestVectorCanonicalTail = ` slutt","enabled":true,"kind":"cron","priority":3,"title":"Ukentlig \u003crapport\u003e \u0026 oppsummering","x\u003cy\u0026z":"nøkkel"}`

const orchestratorDigestVectorCanonical = orchestratorDigestVectorCanonicalHead + "\x7f" + orchestratorDigestVectorCanonicalTail

const orchestratorDigestVectorDigest = "sha256:9a15f77fa517dc15cc4a0933b824a31449dbdad178b286dd74d58c573615cd61"

// TestScheduledTemplateGoalAndPolicyAcceptsTheBFFDigestOfTheSharedVector is the
// last of the three places the template commitment is checked. The template it
// receives is the dispatcher's canonical form, and the digest is the one the BFF
// asked Control to commit to at create.
func TestScheduledTemplateGoalAndPolicyAcceptsTheBFFDigestOfTheSharedVector(t *testing.T) {
	goal, policy, err := scheduledTemplateGoalAndPolicy(orchestratorDigestVectorCanonical, orchestratorDigestVectorDigest)
	if err != nil {
		t.Fatalf("scheduledTemplateGoalAndPolicy: %v", err)
	}
	if policy != "" {
		t.Fatalf("policy = %q, want empty", policy)
	}
	// The goal is the decoded text, not the escaped form: \u escapes come back
	// as the characters they stand for.
	for _, want := range []string{"Ukentlig <rapport> & oppsummering\n\n", "<b>fet</b> > C.", "\u2028to\u2029tre"} {
		if !strings.Contains(goal, want) {
			t.Fatalf("goal %q does not contain %q", goal, want)
		}
	}
}

// TestScheduledTemplateGoalAndPolicyRefusesADigestOfUnescapedBytes is the
// failure the BFF fix prevents: a digest of the same template serialised
// without Go's escaping (what serde_json writes) is a different commitment and
// the run must be refused. The Go side is correct to refuse it, which is why the
// fix is on the BFF side.
func TestScheduledTemplateGoalAndPolicyRefusesADigestOfUnescapedBytes(t *testing.T) {
	unescaped := strings.NewReplacer(
		`\u003c`, "<", `\u003e`, ">", `\u0026`, "&", `\u2028`, "\u2028", `\u2029`, "\u2029",
	).Replace(orchestratorDigestVectorCanonical)
	if unescaped == orchestratorDigestVectorCanonical {
		t.Fatal("the vector must contain characters Go escapes")
	}
	unescapedDigest := fmt.Sprintf("sha256:%x", sha256.Sum256([]byte(unescaped)))

	if _, _, err := scheduledTemplateGoalAndPolicy(orchestratorDigestVectorCanonical, unescapedDigest); err == nil {
		t.Fatal("accepted a digest computed over unescaped template bytes")
	} else if !strings.Contains(err.Error(), "template digest mismatch") {
		t.Fatalf("unexpected error: %v", err)
	}
}
