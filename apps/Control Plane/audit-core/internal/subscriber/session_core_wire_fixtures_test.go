package subscriber

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/triodelab/controlplane/audit-core/internal/events"
)

// The fixtures are VENDORED into this module (internal/testdata/session-core-wire,
// each file with a .sha256 pin beside it), so this test runs from audit-core's own
// Docker build context and reads nothing outside it. The canonical copy is
// session-core's (Model Plane, tests/fixtures/audit-core-wire), regenerated from its
// real builder with UPDATE_AUDIT_CORE_FIXTURES=1; copy both files over, refresh the
// pins, and update the digests in scripts/tests/zdr-contract-sync.test.sh.
//
//   - TestSessionCoreWireFixturesMatchTheirPins fails when a vendored file was edited
//     without its pin, and is always run: the files are inside this module.
//   - TestSessionCoreWireFixturesMatchSessionCoresCanonicalCopy fails when the
//     vendored copy differs from the canonical one. The Model Plane tree sits outside
//     this module, so without REQUIRE_CONTRACT its absence skips; with it, the absence
//     fails: a contract that quietly skips is not a contract.
const (
	sessionCoreWireDir          = "../testdata/session-core-wire"
	sessionCoreCanonicalWireDir = "../../../../Model Plane/rust/services/session-core/tests/fixtures/audit-core-wire"
)

// sha256LF is the digest every pin uses: the file with CRLF normalised to LF.
func sha256LF(data []byte) string {
	sum := sha256.Sum256(bytes.ReplaceAll(data, []byte("\r\n"), []byte("\n")))
	return hex.EncodeToString(sum[:])
}

func TestSessionCoreWireFixturesMatchTheirPins(t *testing.T) {
	entries, err := os.ReadDir(sessionCoreWireDir)
	if err != nil {
		t.Fatalf("read vendored session-core wire fixtures: %v", err)
	}
	checked := 0
	for _, entry := range entries {
		if !strings.HasSuffix(entry.Name(), ".json") {
			continue
		}
		data, err := os.ReadFile(filepath.Join(sessionCoreWireDir, entry.Name()))
		if err != nil {
			t.Fatal(err)
		}
		pin, err := os.ReadFile(filepath.Join(sessionCoreWireDir, entry.Name()+".sha256"))
		if err != nil {
			t.Fatalf("fixture %s has no sha256 pin: %v", entry.Name(), err)
		}
		fields := strings.Fields(string(pin))
		if len(fields) == 0 || fields[0] != sha256LF(data) {
			t.Fatalf("vendored fixture %s differs from its pin; regenerate it from session-core's builder, never by hand", entry.Name())
		}
		checked++
	}
	if checked < 2 {
		t.Fatalf("expected the reservation and final-outcome fixtures, found %d", checked)
	}
}

func TestSessionCoreWireFixturesMatchSessionCoresCanonicalCopy(t *testing.T) {
	canonical, err := os.ReadDir(sessionCoreCanonicalWireDir)
	if err != nil {
		if os.Getenv("REQUIRE_CONTRACT") != "" {
			t.Fatalf("REQUIRE_CONTRACT is set but session-core's fixtures are absent: %v", err)
		}
		t.Skipf("%s is absent (set REQUIRE_CONTRACT=1 to make that a failure)", sessionCoreCanonicalWireDir)
	}
	compared := 0
	for _, entry := range canonical {
		if !strings.HasSuffix(entry.Name(), ".json") {
			continue
		}
		want, err := os.ReadFile(filepath.Join(sessionCoreCanonicalWireDir, entry.Name()))
		if err != nil {
			t.Fatal(err)
		}
		got, err := os.ReadFile(filepath.Join(sessionCoreWireDir, entry.Name()))
		if err != nil {
			t.Fatalf("session-core publishes %s but audit-core has no vendored copy: %v", entry.Name(), err)
		}
		if sha256LF(got) != sha256LF(want) {
			t.Fatalf("vendored %s differs from session-core's canonical copy; copy both files over and refresh the pins", entry.Name())
		}
		compared++
	}
	if compared < 2 {
		t.Fatalf("expected the reservation and final-outcome fixtures, compared %d", compared)
	}
}

// The Model Plane's session-core publishes its governed tool actions ("Used by
// AI?") to this service. Its unit tests pin the exact bytes and subjects its own
// builder produces into committed fixtures; this test feeds the same files to
// audit-core's real decoder and real authority rule. If either side changes the
// body or the subject, one of the two test suites fails before a deploy does.
//
// Why it matters: audit-core accepts a message only when the subject's last
// token equals the payload's `event` (eventAuthorityMatches), and dead-letters
// anything else. session-core once published its reservation event
// (`tool_action_reserved`) on the `tool_action` subject, which this rule refuses.

type sessionCoreFixture struct {
	Subject string          `json:"subject"`
	Body    json.RawMessage `json:"body"`
}

func loadSessionCoreFixtures(t *testing.T) map[string]sessionCoreFixture {
	t.Helper()
	dir := sessionCoreWireDir
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatalf("read session-core wire fixtures: %v", err)
	}
	fixtures := make(map[string]sessionCoreFixture, len(entries))
	for _, entry := range entries {
		if !strings.HasSuffix(entry.Name(), ".json") {
			continue
		}
		raw, err := os.ReadFile(filepath.Join(dir, entry.Name()))
		if err != nil {
			t.Fatal(err)
		}
		var fixture sessionCoreFixture
		if err := json.Unmarshal(raw, &fixture); err != nil {
			t.Fatalf("fixture %s: %v", entry.Name(), err)
		}
		fixtures[entry.Name()] = fixture
	}
	if _, ok := fixtures["tool_action_final.json"]; !ok {
		t.Fatal("missing the final-outcome fixture")
	}
	if _, ok := fixtures["tool_action_reserved.json"]; !ok {
		t.Fatal("missing the reservation fixture")
	}
	return fixtures
}

func TestSessionCoreWireFixturesAreAcceptedByAuditCore(t *testing.T) {
	for name, fixture := range loadSessionCoreFixtures(t) {
		t.Run(name, func(t *testing.T) {
			event, err := events.DecodeAudit(fixture.Body)
			if err != nil {
				t.Fatalf("audit-core refuses the body session-core publishes: %v", err)
			}
			if !eventAuthorityMatches("audit", fixture.Subject, event.Plane, event.Producer, event.Event, "model") {
				t.Fatalf("audit-core's authority rule refuses subject %q for event %q (plane %q, producer %q): "+
					"the message would be dead-lettered as authority_mismatch",
					fixture.Subject, event.Event, event.Plane, event.Producer)
			}
			if event.EventID == "" {
				t.Fatal("no event_id: audit-core could not de-duplicate a replay of this event")
			}
		})
	}
}

// The legacy case, kept as a test of audit-core's rule: a reservation body on
// the final-outcome subject. session-core stored 2,477 such rows before it named
// each phase's subject from the body's event; it now republishes them on the
// right subject, because audit-core refuses them as they were stored.
func TestAuditCoreRefusesAReservationPublishedOnTheFinalOutcomeSubject(t *testing.T) {
	fixtures := loadSessionCoreFixtures(t)
	reserved, err := events.DecodeAudit(fixtures["tool_action_reserved.json"].Body)
	if err != nil {
		t.Fatal(err)
	}
	finalSubject := fixtures["tool_action_final.json"].Subject
	if eventAuthorityMatches("audit", finalSubject, reserved.Plane, reserved.Producer, reserved.Event, "model") {
		t.Fatalf("audit-core accepted event %q on subject %q: its authority rule no longer ties the subject to the event, "+
			"which the replay procedure and session-core's subject derivation rely on", reserved.Event, finalSubject)
	}
}
