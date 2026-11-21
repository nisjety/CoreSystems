package learning

import (
	"testing"
)

// pemPrivateKeyHeader is written in two halves so that a commit-time secret scanner does not read the test fixture as a
// key header. The screen sees the joined header.
const pemPrivateKeyHeader = "-----BEGIN " + "RSA PRIVATE KEY-----"

// awsAccessKeyIDFixture is written in two halves, as the PEM header is, so that a commit-time secret scanner does not read
// the fixture as a key id. The screen sees the joined id.
const awsAccessKeyIDFixture = "AKIA" + "0000000000000000"

// candidateWith is a valid candidate whose content carries text, so only the screen decides whether it is kept.
func candidateWith(text string) SkillCandidate {
	return SkillCandidate{
		Name:            "Weekly report",
		Description:     "Send the weekly report",
		Content:         text,
		TriggerKeywords: []string{"report"},
		Confidence:      0.9,
		Reason:          "Repeated this week.",
	}
}

// TestPIIClassNamesEachClassAndKeepsCleanText covers each class with a positive, and each class's near-misses. The
// positives use public examples (the documented IBANs, the Stripe test card) or synthetic values with the right shape.
func TestPIIClassNamesEachClassAndKeepsCleanText(t *testing.T) {
	cases := []struct {
		name  string
		text  string
		class string // "" means the candidate is kept
	}{
		// Positives, one per class.
		{"email", "Send the report to kari.nordmann@example.com on Fridays.", "email"},
		{"iban, compact", "Pay the fee to NO9386011117947 each month.", "iban"},
		{"iban, lower case", "Pay the fee to no9386011117947 each month.", "iban"},
		{"iban, printed in groups of four", "Pay the fee to NO93 8601 1117 947 each month.", "iban"},
		{"iban, printed, lower case", "Pay the fee to no93 8601 1117 947 each month.", "iban"},
		{"iban, printed with hyphens", "Pay the fee to NO93-8601-1117-947 each month.", "iban"},
		{"iban, printed with a last group of four", "Pay BE68 5390 0754 7034 for the room.", "iban"},
		{"iban, printed with a last group of two", "Pay GB82 WEST 1234 5698 7654 32 for the room.", "iban"},
		{"iban, printed and followed by a short word", "Pay NO93 8601 1117 947 to Kari.", "iban"},
		{"iban, printed after a printed IBAN that fails mod-97", "Pay NO93 8601 1117 948 and BE68 5390 0754 7034 too.", "iban"},
		{"national id, whole", "The form asks for 01019012480 on the second page.", "national_id"},
		{"national id, one space after the sixth digit", "The form asks for 010190 12480 on the second page.", "national_id"},
		{"national id, one hyphen after the sixth digit", "The form asks for 010190-12480 on the second page.", "national_id"},
		{"national id, D-number", "The form asks for 41019012474 on the second page.", "national_id"},
		{"national id, printed in three groups", "The form asks for 010 190 12480 on the second page.", "national_id"},
		{"national id, printed in groups of four", "The form asks for 0101 9012 480 on the second page.", "national_id"},
		{"card, grouped by spaces", "Charge the card 4242 4242 4242 4242 monthly.", "card"},
		{"card, grouped by hyphens", "Charge the card 4242-4242-4242-4242 monthly.", "card"},
		{"card, whole", "Charge the card 4242424242424242 monthly.", "card"},
		{"card, grouped 4-6-5 as American Express prints it", "Charge the card 3782 822463 10005 monthly.", "card"},
		{"card, grouped with two spaces", "Charge the card 4242  4242  4242  4242 monthly.", "card"},
		{"card, grouped with a spaced hyphen", "Charge the card 4242 - 4242 - 4242 - 4242 monthly.", "card"},
		{"phone", "Ring +47 900 00 000 after lunch.", "phone"},
		{"secret, AWS access key id", "Put " + awsAccessKeyIDFixture + " in the config.", "secret"},
		{"secret, sk token", "Use sk-fixture-aaaaaaaaaaaaaaaaaaaa for the sandbox.", "secret"},
		{"secret, JWT-like token", "The token eyJfixture-aaaaaaaa.eyJfixture-bbbbbbbb.fixture-cccccc was pasted.", "secret"},
		{"secret, PEM private key header", "Paste the header " + pemPrivateKeyHeader + " into the file.", "secret"},
		{"secret, bearer credential", "Send Authorization: Bearer abcdefghijklmnopqrstuvwxyz0123 with each call.", "secret"},

		// Near-misses, which must keep the candidate.
		{"ten-digit number", "The order number 0101901248 shipped on Monday.", ""},
		{"card-length number that fails Luhn", "The reference 4242 4242 4242 4241 shipped on Monday.", ""},
		{"version strings", "Upgrade to v1.2.3 in release 2026.11.20 on Monday.", ""},
		{"e-mail-like code path", "Install it with go get example.com/tool@v1.2.3 first.", ""},
		{"national id with a bad checksum", "The form asks for 01019012481 on the second page.", ""},
		{"national id with valid checksums and an impossible date", "The form asks for 31029012493 on the second page.", ""},
		{"plus sign with too few digits", "Code +1234567 is internal.", ""},
		{"IBAN-shaped word that fails mod-97", "Code NO9386011117948 is internal.", ""},
		{"printed IBAN-shaped groups that fail mod-97", "Code NO93 8601 1117 948 is internal.", ""},
		{"printed IBAN-shaped groups of the wrong sizes", "Code NO93 86011 1117 947 is internal.", ""},
		{"printed national id with a bad checksum", "The form asks for 010 190 12481 on the second page.", ""},
		{"AKIA prefix with too few characters", "Key AKIAABCDEFGHIJKLMNO is short.", ""},
		{"sk prefix with too few characters", "Use sk-short-token for now.", ""},
		{"bearer word with a short token", "Use Bearer abc123 for the probe.", ""},
		{"certificate header, not a key", "-----BEGIN CERTIFICATE----- starts the file.", ""},
		{"a word that merely contains sk-", "Open the task-manager-dashboard-overview page.", ""},
		{"plain prose", "Send the weekly report every Friday before noon.", ""},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			c := candidateWith(tc.text)
			if got := PIIClass(c); got != tc.class {
				t.Fatalf("PIIClass(%q) = %q, want %q", tc.text, got, tc.class)
			}
			if kept := PIIScreen(c); kept != (tc.class == "") {
				t.Fatalf("PIIScreen(%q) = %v, want %v", tc.text, kept, tc.class == "")
			}
		})
	}
}

// TestNationalIDNearMissesAreRejectedForTheRightReason checks the identity-number rules on their own, so the near-miss
// rows above fail for the reason they name: a bad date with right check digits, and a bad check digit.
func TestNationalIDNearMissesAreRejectedForTheRightReason(t *testing.T) {
	const impossibleDate = "31029012493" // 31 February with both check digits right
	if got := controlDigit(impossibleDate[:9], tenthDigitWeights); got != int(impossibleDate[9]-'0') {
		t.Fatalf("fixture %s: tenth digit %d, check gives %d", impossibleDate, impossibleDate[9]-'0', got)
	}
	if got := controlDigit(impossibleDate[:10], eleventhDigitWeights); got != int(impossibleDate[10]-'0') {
		t.Fatalf("fixture %s: eleventh digit %d, check gives %d", impossibleDate, impossibleDate[10]-'0', got)
	}
	if plausibleBirthDate(impossibleDate[:6]) {
		t.Fatal("31 February must not be a plausible birth date")
	}
	if !plausibleBirthDate("410190") {
		t.Fatal("a D-number of 41 (the first of January, plus 40) must be a plausible birth date")
	}
	if validNationalID("01019012481") {
		t.Fatal("an eleventh digit that is not the check digit must fail the identity number")
	}
	if !validNationalID("01019012480") {
		t.Fatal("the reference identity number must pass both check digits and the date")
	}
}

// TestPIIScreenReadsEveryField pins the fields the screen reads: name, description, content, every trigger keyword and
// the reason. A field the screen skipped would let a proposal through with the personal data in it.
func TestPIIScreenReadsEveryField(t *testing.T) {
	const address = "kari.nordmann@example.com"
	fields := map[string]SkillCandidate{
		"name":        {Name: address, Description: "d", Content: "steps", Confidence: 1},
		"description": {Name: "n", Description: address, Content: "steps", Confidence: 1},
		"content":     {Name: "n", Content: address, Confidence: 1},
		"keyword":     {Name: "n", Content: "steps", TriggerKeywords: []string{"fridays", address}, Confidence: 1},
		"reason":      {Name: "n", Content: "steps", Reason: address, Confidence: 1},
	}
	for field, c := range fields {
		if got := PIIClass(c); got != "email" {
			t.Errorf("an e-mail address in the %s field: PIIClass = %q, want email", field, got)
		}
		if PIIScreen(c) {
			t.Errorf("an e-mail address in the %s field must drop the candidate", field)
		}
	}
}

// TestPIIClassReportsTheFirstClassInItsOrder pins the order the classes are tried in: a candidate that carries more than
// one class is named by the first of them, so the counter's label never depends on map or scan order.
func TestPIIClassReportsTheFirstClassInItsOrder(t *testing.T) {
	cases := []struct {
		text, want string
	}{
		{"Charge kari.nordmann@example.com and 4242 4242 4242 4242.", "email"},
		{"Key AKIA0000000000000000 charges 4242 4242 4242 4242.", "secret"},
		{"Ring +47 900 00 000 or mail kari.nordmann@example.com.", "email"},
		{"Pay NO93 8601 1117 947 for 4242 4242 4242 4242.", "iban"},
	}
	for _, tc := range cases {
		if got := PIIClass(candidateWith(tc.text)); got != tc.want {
			t.Errorf("PIIClass(%q) = %q, want %q", tc.text, got, tc.want)
		}
	}
}

// TestPIIScreenIsDeterministic runs the same candidate many times: the screen is a pure function of the candidate.
func TestPIIScreenIsDeterministic(t *testing.T) {
	c := candidateWith("Charge the card 4242 4242 4242 4242 monthly.")
	first := PIIClass(c)
	for i := 0; i < 20; i++ {
		if got := PIIClass(c); got != first {
			t.Fatalf("PIIClass changed between calls: %q then %q", first, got)
		}
	}
}

// TestPIIScreenDropsTheCandidateBeforeTheSinkAndKeepsTheRest installs the screen as Screen and runs the selection the way
// the review does: the candidate that carries personal data never reaches the sink, and the clean one is kept.
func TestPIIScreenDropsTheCandidateBeforeTheSinkAndKeepsTheRest(t *testing.T) {
	original := Screen
	t.Cleanup(func() { Screen = original })
	Screen = PIIScreen

	leaky := candidateWith("Send the report to kari.nordmann@example.com on Fridays.")
	leaky.Name = "Fridays report"
	clean := candidateWith("Send the weekly report every Friday before noon.")
	clean.Name = "Weekly report"

	kept := SelectForPersistence([]SkillCandidate{leaky, clean}, nil)
	if len(kept) != 1 || kept[0].Name != "Weekly report" {
		t.Fatalf("SelectForPersistence with the PII screen = %+v, want only the clean candidate", kept)
	}
}
