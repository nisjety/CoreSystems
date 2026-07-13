package postgres

import "testing"

func TestScopedIdempotencyKeyIncludesTenantWithoutExposingRawKey(t *testing.T) {
	a := scopedIdempotencyKey("org-a", "retry-1")
	b := scopedIdempotencyKey("org-b", "retry-1")
	if a == "" || b == "" || a == b || a == "retry-1" || b == "retry-1" {
		t.Fatalf("keys are not safely tenant-scoped: a=%q b=%q", a, b)
	}
	if got := scopedIdempotencyKey("org-a", ""); got != "" {
		t.Fatalf("empty key=%q want empty", got)
	}
}
