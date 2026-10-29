package store

import (
	"testing"
	"time"

	"github.com/google/uuid"
)

func TestNewestMemberIndexPicksMostRecent(t *testing.T) {
	t.Parallel()

	t1 := time.Date(2024, 1, 1, 0, 0, 0, 0, time.UTC)
	t2 := time.Date(2026, 5, 1, 0, 0, 0, 0, time.UTC) // newest
	t3 := time.Date(2025, 1, 1, 0, 0, 0, 0, time.UTC)

	members := []DuplicateMember{
		{ItemPK: uuid.New(), ModifiedAt: &t1},
		{ItemPK: uuid.New(), ModifiedAt: &t2},
		{ItemPK: uuid.New(), ModifiedAt: &t3},
	}
	if got := newestMemberIndex(members); got != 1 {
		t.Fatalf("newestMemberIndex = %d, want 1", got)
	}
}

func TestNewestMemberIndexPrefersKnownTimestamp(t *testing.T) {
	t.Parallel()

	known := time.Date(2020, 1, 1, 0, 0, 0, 0, time.UTC)
	members := []DuplicateMember{
		{ItemPK: uuid.New(), ModifiedAt: nil},
		{ItemPK: uuid.New(), ModifiedAt: &known},
	}
	if got := newestMemberIndex(members); got != 1 {
		t.Fatalf("newestMemberIndex = %d, want 1 (known beats nil)", got)
	}
}
