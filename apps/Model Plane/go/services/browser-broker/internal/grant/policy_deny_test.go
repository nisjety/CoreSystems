package grant

import (
	"errors"
	"slices"
	"testing"
	"time"
)

// WS7.K: a grant covers the subdomains of each of its hosts, so an org deny rule
// below a host the grant names is a denied host the grant reaches. Narrow,
// Derive and the sensitive approval refuse such a host with ErrDomainDenied, so
// no grant is minted whose allowed_domains name a host its own snapshot denies.

// TestNarrowRefusesAHostThatReachesADeniedHost covers a deny rule below a
// requested host (the grant would cover it) and the allow-list narrowing.
func TestNarrowRefusesAHostThatReachesADeniedHost(t *testing.T) {
	cases := []struct {
		name      string
		allow     []string
		deny      []string
		requested []string
		want      []string // nil: refused with ErrDomainDenied
	}{
		{"parent over a denied host, no allow rule", nil, []string{"evil.example.com"}, []string{"example.com"}, nil},
		{"parent over a denied host, inside the allow rule", []string{"example.com"}, []string{"evil.example.com"}, []string{"example.com"}, nil},
		{"parent narrows to the allowed host beside the deny", []string{"docs.example.com"}, []string{"evil.example.com"}, []string{"example.com"}, []string{"docs.example.com"}},
		{"allowed host over a denied host is never added", []string{"a.example.com", "b.example.com"}, []string{"x.a.example.com"}, []string{"example.com"}, []string{"b.example.com"}},
		{"requested host under a deny rule is refused", nil, []string{"evil.example.com"}, []string{"evil.example.com", "docs.example.com"}, nil},
		{"host beside the deny rule is kept", nil, []string{"evil.example.com"}, []string{"docs.example.com"}, []string{"docs.example.com"}},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			snapshot := &WebPolicySnapshot{V: 1, Rev: "1", DomainsAllow: c.allow, DomainsDeny: c.deny}
			got, err := Narrow(c.requested, snapshot)
			if c.want == nil {
				if !errors.Is(err, ErrDomainDenied) {
					t.Fatalf("Narrow(%v) = %v, %v; want ErrDomainDenied", c.requested, got, err)
				}
				return
			}
			if err != nil || !slices.Equal(got, c.want) {
				t.Fatalf("Narrow(%v) = %v, %v; want %v", c.requested, got, err, c.want)
			}
		})
	}
}

// TestDeriveRefusesAHostTheParentSnapshotDenies: a run grant keeps every deny
// entry its parent was minted with, so Derive refuses a host the parent's
// snapshot denies, whether it is named, covers the denied host, or is inherited.
func TestDeriveRefusesAHostTheParentSnapshotDenies(t *testing.T) {
	store := NewMemoryRepository()
	parent := session(t, store, CreateSessionParams{
		AllowedDomains: []string{"example.com"}, TTL: 15 * time.Minute,
		WebPolicy: &WebPolicySnapshot{V: 1, Rev: "1", DomainsDeny: []string{"evil.example.com"}},
	})
	refused := []struct {
		key     string
		domains []string
	}{
		{"named-denied", []string{"evil.example.com"}},
		{"covering", []string{"example.com"}},
		{"inherited", nil},
	}
	for _, c := range refused {
		if _, err := store.Derive(ctx, deriveParams(parent, c.key, c.domains...)); !errors.Is(err, ErrDomainDenied) {
			t.Fatalf("Derive(%s, %v) error = %v; want ErrDomainDenied", c.key, c.domains, err)
		}
	}
	run, err := store.Derive(ctx, deriveParams(parent, "beside", "docs.example.com"))
	if err != nil || !sameStrings(run.AllowedDomains, []string{"docs.example.com"}) {
		t.Fatalf("Derive beside the deny = %#v, %v", run, err)
	}
}

// TestSensitiveApprovalRefusesAHostTheParentSnapshotDenies: an approval is never
// wider than its parent, and the parent's deny rules hold for it too.
func TestSensitiveApprovalRefusesAHostTheParentSnapshotDenies(t *testing.T) {
	store := NewMemoryRepository()
	parent := session(t, store, CreateSessionParams{
		AllowedDomains: []string{"example.com"}, TTL: 15 * time.Minute,
		WebPolicy: &WebPolicySnapshot{V: 1, Rev: "1", DomainsDeny: []string{"evil.example.com"}},
	})
	for _, domains := range [][]string{{"evil.example.com"}, {"example.com"}} {
		if _, err := store.CreateScopedSensitiveApproval(ctx, approvalOn(parent, domains, time.Minute)); !errors.Is(err, ErrDomainDenied) {
			t.Fatalf("approval for %v error = %v; want ErrDomainDenied", domains, err)
		}
	}
	approval, err := store.CreateScopedSensitiveApproval(ctx, approvalOn(parent, []string{"docs.example.com"}, time.Minute))
	if err != nil || !sameStrings(approval.AllowedDomains, []string{"docs.example.com"}) {
		t.Fatalf("approval beside the deny = %#v, %v", approval, err)
	}
}

// A fill_otp origin is typed into, so the parent's deny rules hold for it as they
// hold for the approval's hosts. A denied origin is refused whether or not the
// approval's own hosts name it; the domains alone refuse the origin they cover.
func TestFillOtpApprovalRefusesAnOriginTheParentSnapshotDenies(t *testing.T) {
	store := NewMemoryRepository()
	parent := session(t, store, CreateSessionParams{
		AllowedDomains: []string{"example.com"}, TTL: 15 * time.Minute,
		WebPolicy: &WebPolicySnapshot{V: 1, Rev: "1", DomainsDeny: []string{"evil.example.com"}},
	})
	otp := func(domains []string, origin string) ApprovalParams {
		p := approvalOn(parent, domains, time.Minute)
		p.AllowedActions = []string{ActionFillOtp}
		p.MailConnectionID = "mail-1"
		p.GrantOrigin = origin
		return p
	}
	refused := []struct {
		domains []string
		origin  string
	}{
		{[]string{"example.com"}, "https://evil.example.com"},
		{[]string{"docs.example.com"}, "https://evil.example.com"},
	}
	for _, c := range refused {
		if _, err := store.CreateScopedSensitiveApproval(ctx, otp(c.domains, c.origin)); !errors.Is(err, ErrDomainDenied) {
			t.Fatalf("fill_otp approval for %v origin %s error = %v; want ErrDomainDenied", c.domains, c.origin, err)
		}
	}
	approval, err := store.CreateScopedSensitiveApproval(ctx, otp([]string{"docs.example.com"}, "https://docs.example.com"))
	if err != nil || approval.GrantOrigin != "https://docs.example.com" {
		t.Fatalf("fill_otp origin beside the deny = %#v, %v", approval, err)
	}
}
