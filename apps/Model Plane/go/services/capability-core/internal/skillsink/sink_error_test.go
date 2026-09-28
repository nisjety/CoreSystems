package skillsink

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
)

// A failed upsert surfaces in the review consumer's retry log, so its error
// must not quote the generated skill.
func TestPersist_RpcErrorNamesPositionNotSkill(t *testing.T) {
	sink := NewSessionCoreSink(&fakeUpserter{err: errors.New("rpc down")}, "org1")
	_, err := sink.Persist(context.Background(), []learning.SkillCandidate{
		{Name: "Quote for customer-secret-7731", Content: "c"},
	})
	if err == nil || !strings.Contains(err.Error(), "upsert skill 1 of 1") || !strings.Contains(err.Error(), "rpc down") {
		t.Fatalf("want a positional upsert error wrapping the cause, got %v", err)
	}
	if strings.Contains(err.Error(), "customer-secret-7731") {
		t.Fatalf("upsert error quotes the generated skill name: %v", err)
	}
}
