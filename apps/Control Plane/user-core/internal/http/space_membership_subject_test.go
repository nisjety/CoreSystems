package http

import (
	"os"
	"regexp"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS04-S10: the inline publishers (fast path) and the durable outbox must use
// one subject, the one Session Core subscribes to. The shared publisher's
// helpers are scanned as text because they have no seam to observe the subject
// without a broker.
func TestMembershipPublishersAllUseTheSharedSubject(t *testing.T) {
	const subject = "controlplane.space.membership_changed"
	if spaces.MembershipChangedSubject != subject {
		t.Fatalf("outbox subject = %q, want %q", spaces.MembershipChangedSubject, subject)
	}
	raw, err := os.ReadFile("../nats/shared_publisher.go")
	if err != nil {
		t.Fatalf("read shared publisher: %v", err)
	}
	src := strings.ReplaceAll(string(raw), "\r\n", "\n")
	for _, name := range []string{"PublishSpaceMembershipChanged", "PublishSpaceMembershipEvent"} {
		re := regexp.MustCompile(`(?s)func \(sp \*SharedPublisher\) ` + name + `\(.*?\n}\n`)
		body := re.FindString(src)
		if body == "" {
			t.Fatalf("%s not found", name)
		}
		if !strings.Contains(body, `"`+subject+`"`) {
			t.Errorf("%s does not publish on %s", name, subject)
		}
	}
	// The additive event keeps the current-fact fields of the original.
	event := regexp.MustCompile(`(?s)func \(sp \*SharedPublisher\) PublishSpaceMembershipEvent\(.*?\n}\n`).FindString(src)
	for _, field := range []string{"space_ref", "org_id", "subject_id", "active", "event_id", "space_seq", "action", "role", "actor_kind"} {
		if !strings.Contains(event, `"`+field+`"`) {
			t.Errorf("PublishSpaceMembershipEvent lost the %q field", field)
		}
	}
	// The durable path is the one that waits for an ack.
	if !strings.Contains(src, "func (sp *SharedPublisher) PublishWithAck(") {
		t.Error("PublishWithAck is gone")
	}
}
