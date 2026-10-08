package ledger

import (
	"context"
	"errors"
	"testing"
)

func TestErasedTenantCannotBeRestoredByUsageReplay(t *testing.T) {
	s := NewStore()
	entry := Entry{OrgID: "erased-org", UserID: "user-a", InputTokens: 12, IdempotencyKey: "request-a"}
	if err := s.RecordEntry(context.Background(), entry); err != nil {
		t.Fatal(err)
	}
	if err := s.PurgeOrg(context.Background(), entry.OrgID); err != nil {
		t.Fatal(err)
	}
	if err := s.RecordEntry(context.Background(), entry); !errors.Is(err, ErrOrgErased) {
		t.Fatalf("replay err=%v", err)
	}
	entry.IdempotencyKey = "request-b"
	if err := s.RecordEntry(context.Background(), entry); !errors.Is(err, ErrOrgErased) {
		t.Fatalf("new erased-org entry err=%v", err)
	}
	entry.OrgID = "other-org"
	if err := s.RecordEntry(context.Background(), entry); err != nil {
		t.Fatal(err)
	}
}
