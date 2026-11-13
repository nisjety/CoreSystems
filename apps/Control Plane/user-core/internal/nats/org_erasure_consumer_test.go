package nats

import (
	"bytes"
	"context"
	"errors"
	"log"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

// FU-27: user-core erases a purged organisation's resource grants on org-core's
// verevon.gdpr.erasure.requested fan-out (subject_type organization only).

type fakeEraser struct {
	calls []string
	err   error
	count int64
}

func (f *fakeEraser) DeleteGrantsForOrg(_ context.Context, orgID string) (int64, error) {
	f.calls = append(f.calls, orgID)
	return f.count, f.err
}

func captureLog(t *testing.T) *bytes.Buffer {
	t.Helper()
	var buf bytes.Buffer
	prev, prevFlags := log.Writer(), log.Flags()
	log.SetOutput(&buf)
	t.Cleanup(func() {
		log.SetOutput(prev)
		log.SetFlags(prevFlags)
	})
	return &buf
}

func TestOrgErasureConsumerActsOnOrganizationEventsOnly(t *testing.T) {
	for _, subjectType := range []string{"user", "user_anonymize", "", "Organization", "org"} {
		eraser := &fakeEraser{}
		c := NewOrgErasureConsumer(nil, eraser)
		retry := c.process(context.Background(), orgErasureEvent{SubjectType: subjectType, SubjectID: "org-a", OrgID: "org-a"}, "seq-1")
		if retry {
			t.Errorf("subject_type %q: retry = true, want a definitive no-op", subjectType)
		}
		if len(eraser.calls) != 0 {
			t.Errorf("subject_type %q: deleted grants %v, want none", subjectType, eraser.calls)
		}
	}
}

func TestOrgErasureConsumerDeletesTheOrgGrants(t *testing.T) {
	eraser := &fakeEraser{count: 3}
	c := NewOrgErasureConsumer(nil, eraser)
	if retry := c.process(context.Background(), orgErasureEvent{SubjectType: "organization", SubjectID: "org-subject", OrgID: "org-a"}, "seq-1"); retry {
		t.Fatal("retry = true on success")
	}
	if len(eraser.calls) != 1 || eraser.calls[0] != "org-a" {
		t.Fatalf("calls = %v, want [org-a] (org_id wins over subject_id)", eraser.calls)
	}
}

func TestOrgErasureConsumerFallsBackToSubjectID(t *testing.T) {
	eraser := &fakeEraser{}
	c := NewOrgErasureConsumer(nil, eraser)
	c.process(context.Background(), orgErasureEvent{SubjectType: "organization", SubjectID: " org-b ", OrgID: "  "}, "seq-1")
	if len(eraser.calls) != 1 || eraser.calls[0] != "org-b" {
		t.Fatalf("calls = %v, want [org-b]", eraser.calls)
	}
}

func TestOrgErasureConsumerSkipsAnOrgEventWithoutAnID(t *testing.T) {
	eraser := &fakeEraser{}
	c := NewOrgErasureConsumer(nil, eraser)
	if retry := c.process(context.Background(), orgErasureEvent{SubjectType: "organization"}, "seq-1"); retry {
		t.Fatal("an event without an org id must not be retried forever")
	}
	if len(eraser.calls) != 0 {
		t.Fatalf("calls = %v, want none", eraser.calls)
	}
}

func TestOrgErasureConsumerRetriesOnADeleteError(t *testing.T) {
	eraser := &fakeEraser{err: errors.New("db down")}
	c := NewOrgErasureConsumer(nil, eraser)
	if retry := c.process(context.Background(), orgErasureEvent{SubjectType: "organization", OrgID: "org-a"}, "seq-1"); !retry {
		t.Fatal("a failed delete must be retried (nak), never acked")
	}
}

func TestOrgErasureConsumerLogsOnlyIDsAndCounts(t *testing.T) {
	buf := captureLog(t)
	eraser := &fakeEraser{count: 2}
	c := NewOrgErasureConsumer(nil, eraser)
	c.process(context.Background(), orgErasureEvent{SubjectType: "organization", OrgID: "org-a", SubjectID: "org-a"}, "seq-7")
	out := buf.String()
	if !strings.Contains(out, "org=org-a") || !strings.Contains(out, "event=seq-7") || !strings.Contains(out, "deleted=2") {
		t.Fatalf("log = %q, want org id, event id and count", out)
	}
}

func TestOrgErasureConsumerStartRefusesWithoutDependencies(t *testing.T) {
	if err := NewOrgErasureConsumer(nil, &fakeEraser{}).Start(context.Background()); err == nil {
		t.Fatal("Start without a JetStream context must fail")
	}
	var nilConsumer *OrgErasureConsumer
	nilConsumer.Stop() // must not panic
}

// The consumer binds (never creates) the durable audit-core provisions. A name
// drift breaks the org purge silently, so pin the two modules together.
func TestOrgErasureDurableMatchesAuditCoreProvisioner(t *testing.T) {
	path := filepath.Join("..", "..", "..", "audit-core", "internal", "provisioner", "provisioner.go")
	data, err := os.ReadFile(path)
	if err != nil {
		t.Skipf("audit-core provisioner not in this tree: %v", err)
	}
	m := regexp.MustCompile(`UserCoreOrgErasureConsumerName\s*=\s*"([^"]+)"`).FindSubmatch(data)
	if m == nil {
		t.Fatal("UserCoreOrgErasureConsumerName not found in audit-core provisioner.go")
	}
	if string(m[1]) != OrgErasureDurable {
		t.Fatalf("audit-core durable %q, user-core binds %q", m[1], OrgErasureDurable)
	}
}

// The switch is the strict literal "true", read in cmd/server/main.go.
func TestOrgErasureSwitchIsTheStrictLiteral(t *testing.T) {
	path := filepath.Join("..", "..", "cmd", "server", "main.go")
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read main.go: %v", err)
	}
	if !strings.Contains(string(data), `os.Getenv("USER_CORE_ORG_ERASURE_CONSUMER_ENABLED") == "true"`) {
		t.Fatal(`USER_CORE_ORG_ERASURE_CONSUMER_ENABLED must be read as os.Getenv(...) == "true"`)
	}
}
