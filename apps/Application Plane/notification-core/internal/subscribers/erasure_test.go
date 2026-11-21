package subscribers

import (
	"context"
	"errors"
	"strings"
	"testing"
)

// fakeMembershipLister returns the rows it is given, exactly as a store would, and records each organization and user it is
// asked about.
type fakeMembershipLister struct {
	orgIDs      []string
	userIDs     []string
	memberships []Membership
	err         error
}

func (f *fakeMembershipLister) ListMembershipsForOrganization(_ context.Context, organizationID string) ([]Membership, error) {
	f.orgIDs = append(f.orgIDs, organizationID)
	if f.err != nil {
		return nil, f.err
	}
	return append([]Membership(nil), f.memberships...), nil
}

func (f *fakeMembershipLister) ListMembershipsForUser(_ context.Context, userID string) ([]Membership, error) {
	f.userIDs = append(f.userIDs, userID)
	if f.err != nil {
		return nil, f.err
	}
	return append([]Membership(nil), f.memberships...), nil
}

// fakeProviderIdentities behaves like Novu for the erasure: a subscriber holds messages, can be removed once, and a
// removed one answers "already gone". It records every call in order.
type fakeProviderIdentities struct {
	calls         []string
	messages      map[string]int
	gone          map[string]bool
	messagesErr   map[string]error
	subscriberErr map[string]error
}

func newFakeProviderIdentities() *fakeProviderIdentities {
	return &fakeProviderIdentities{
		messages:      map[string]int{},
		gone:          map[string]bool{},
		messagesErr:   map[string]error{},
		subscriberErr: map[string]error{},
	}
}

func (f *fakeProviderIdentities) DeleteSubscriberMessages(_ context.Context, subscriberID string) (int, error) {
	f.calls = append(f.calls, "messages:"+subscriberID)
	if err := f.messagesErr[subscriberID]; err != nil {
		return 0, err
	}
	removed := f.messages[subscriberID]
	f.messages[subscriberID] = 0
	return removed, nil
}

func (f *fakeProviderIdentities) DeleteSubscriber(_ context.Context, subscriberID string) (bool, error) {
	f.calls = append(f.calls, "subscriber:"+subscriberID)
	if err := f.subscriberErr[subscriberID]; err != nil {
		return false, err
	}
	if f.gone[subscriberID] {
		return false, nil
	}
	f.gone[subscriberID] = true
	return true, nil
}

func membershipFor(organizationID, userID string, status string) Membership {
	return Membership{
		OrganizationID:       organizationID,
		UserID:               userID,
		ProviderSubscriberID: providerSubscriberID(organizationID, userID),
		Status:               status,
	}
}

// Every membership of the organization is erased, active and removed alike. Messages go before the subscriber, so the
// listing still finds them.
func TestProviderErasureDeletesEachMembershipsMessagesThenItsSubscriber(t *testing.T) {
	lister := &fakeMembershipLister{memberships: []Membership{
		membershipFor("org-a", "user-1", MembershipStatusActive),
		membershipFor("org-a", "user-2", MembershipStatusRemoved),
	}}
	provider := newFakeProviderIdentities()
	provider.messages[providerSubscriberID("org-a", "user-1")] = 3
	provider.messages[providerSubscriberID("org-a", "user-2")] = 1
	eraser := NewProviderEraser(lister, provider)

	counts, err := eraser.PurgeOrganization(context.Background(), "org-a")
	if err != nil {
		t.Fatalf("PurgeOrganization() error = %v", err)
	}
	if len(lister.orgIDs) != 1 || lister.orgIDs[0] != "org-a" {
		t.Fatalf("lister asked about %v, want exactly org-a", lister.orgIDs)
	}
	first, second := providerSubscriberID("org-a", "user-1"), providerSubscriberID("org-a", "user-2")
	want := []string{"messages:" + first, "subscriber:" + first, "messages:" + second, "subscriber:" + second}
	if strings.Join(provider.calls, "|") != strings.Join(want, "|") {
		t.Fatalf("provider calls = %v, want %v", provider.calls, want)
	}
	if counts["novu_subscriber_messages"] != 4 || counts["novu_subscribers"] != 2 {
		t.Fatalf("counts = %v, want 4 messages and 2 subscribers", counts)
	}
}

// A user who belongs to two organizations has two provider subscribers, one per membership. Erasing one organization
// removes only its own subscriber, and the other organization's identity for the same user survives.
func TestProviderErasureNeverTouchesAnotherOrganizationsIdentityForTheSameUser(t *testing.T) {
	orgA := providerSubscriberID("org-a", "user-1")
	orgB := providerSubscriberID("org-b", "user-1")
	if orgA == orgB {
		t.Fatal("the same user has one provider subscriber in two organizations, so erasure could reach the other one")
	}
	// The lister is handed rows for both organizations, as a faulty store might return them. The eraser must still refuse
	// the other organization's row.
	lister := &fakeMembershipLister{memberships: []Membership{
		membershipFor("org-a", "user-1", MembershipStatusActive),
		membershipFor("org-b", "user-1", MembershipStatusActive),
	}}
	provider := newFakeProviderIdentities()
	eraser := NewProviderEraser(lister, provider)

	counts, err := eraser.PurgeOrganization(context.Background(), "org-a")
	if err != nil {
		t.Fatalf("PurgeOrganization() error = %v", err)
	}
	for _, call := range provider.calls {
		if strings.Contains(call, orgB) {
			t.Fatalf("provider call %q reached another organization's subscriber", call)
		}
	}
	if counts["novu_subscribers"] != 1 {
		t.Fatalf("counts = %v, want exactly one subscriber removed", counts)
	}
}

// A replayed erasure, or one whose subscriber was already removed, removes nothing and is not an error.
func TestProviderErasureTreatsAnAlreadyRemovedSubscriberAsDone(t *testing.T) {
	lister := &fakeMembershipLister{memberships: []Membership{membershipFor("org-a", "user-1", MembershipStatusActive)}}
	provider := newFakeProviderIdentities()
	eraser := NewProviderEraser(lister, provider)

	if _, err := eraser.PurgeOrganization(context.Background(), "org-a"); err != nil {
		t.Fatalf("first PurgeOrganization() error = %v", err)
	}
	counts, err := eraser.PurgeOrganization(context.Background(), "org-a")
	if err != nil {
		t.Fatalf("replayed PurgeOrganization() error = %v, want nil for an already-removed subscriber", err)
	}
	if counts["novu_subscribers"] != 0 || counts["novu_subscriber_messages"] != 0 {
		t.Fatalf("replayed counts = %v, want zero", counts)
	}
}

// A message that cannot be deleted stops the erasure before the subscriber is touched, so the retry starts again from the
// messages.
func TestProviderErasureStopsBeforeTheSubscriberWhenMessagesFail(t *testing.T) {
	subscriber := providerSubscriberID("org-a", "user-1")
	lister := &fakeMembershipLister{memberships: []Membership{membershipFor("org-a", "user-1", MembershipStatusActive)}}
	provider := newFakeProviderIdentities()
	provider.messagesErr[subscriber] = errors.New("provider refused the message delete")
	eraser := NewProviderEraser(lister, provider)

	if _, err := eraser.PurgeOrganization(context.Background(), "org-a"); err == nil {
		t.Fatal("PurgeOrganization() error = nil, want the message failure")
	}
	for _, call := range provider.calls {
		if strings.HasPrefix(call, "subscriber:") {
			t.Fatalf("subscriber %q was deleted after its messages failed: calls = %v", subscriber, provider.calls)
		}
	}
}

func TestProviderErasureReturnsASubscriberFailure(t *testing.T) {
	subscriber := providerSubscriberID("org-a", "user-1")
	lister := &fakeMembershipLister{memberships: []Membership{membershipFor("org-a", "user-1", MembershipStatusActive)}}
	provider := newFakeProviderIdentities()
	provider.subscriberErr[subscriber] = errors.New("provider unavailable")
	eraser := NewProviderEraser(lister, provider)

	if _, err := eraser.PurgeOrganization(context.Background(), "org-a"); err == nil {
		t.Fatal("PurgeOrganization() error = nil, want the subscriber failure so the message retries")
	}
}

// An empty organization id is refused before anything is listed or deleted, so it can never widen into a sweep.
func TestProviderErasureRefusesAnEmptyOrganization(t *testing.T) {
	lister := &fakeMembershipLister{memberships: []Membership{membershipFor("org-a", "user-1", MembershipStatusActive)}}
	provider := newFakeProviderIdentities()
	eraser := NewProviderEraser(lister, provider)

	if _, err := eraser.PurgeOrganization(context.Background(), "   "); !errors.Is(err, errProviderEraseOrganizationRequired) {
		t.Fatalf("PurgeOrganization(empty) error = %v, want the organization-required refusal", err)
	}
	if len(lister.orgIDs) != 0 || len(provider.calls) != 0 {
		t.Fatalf("empty organization listed %v and called %v, want nothing", lister.orgIDs, provider.calls)
	}
}

// A membership row with no provider identity has no subscriber to erase, so it is skipped rather than sent to the provider
// with an empty id, which would address the collection instead of one subscriber.
func TestProviderErasureSkipsAMembershipWithNoProviderIdentity(t *testing.T) {
	lister := &fakeMembershipLister{memberships: []Membership{{OrganizationID: "org-a", UserID: "user-1", Status: MembershipStatusActive}}}
	provider := newFakeProviderIdentities()
	eraser := NewProviderEraser(lister, provider)

	counts, err := eraser.PurgeOrganization(context.Background(), "org-a")
	if err != nil {
		t.Fatalf("PurgeOrganization() error = %v", err)
	}
	if len(provider.calls) != 0 {
		t.Fatalf("provider calls = %v, want none for an empty provider identity", provider.calls)
	}
	if counts["novu_subscribers"] != 0 {
		t.Fatalf("counts = %v, want zero", counts)
	}
}

func TestProviderErasureListFailureIsReturnedBeforeAnyProviderCall(t *testing.T) {
	lister := &fakeMembershipLister{err: errors.New("database unavailable")}
	provider := newFakeProviderIdentities()
	eraser := NewProviderEraser(lister, provider)

	if _, err := eraser.PurgeOrganization(context.Background(), "org-a"); err == nil {
		t.Fatal("PurgeOrganization() error = nil, want the listing failure")
	}
	if len(provider.calls) != 0 {
		t.Fatalf("provider calls = %v after a listing failure, want none", provider.calls)
	}
}

func TestProviderErasureRefusesToRunWithoutItsDependencies(t *testing.T) {
	var nilEraser *ProviderEraser
	if _, err := nilEraser.PurgeOrganization(context.Background(), "org-a"); err == nil {
		t.Fatal("nil eraser PurgeOrganization() error = nil, want not configured")
	}
	if _, err := NewProviderEraser(nil, newFakeProviderIdentities()).PurgeOrganization(context.Background(), "org-a"); err == nil {
		t.Fatal("eraser without a membership store error = nil, want not configured")
	}
	if _, err := NewProviderEraser(&fakeMembershipLister{}, nil).PurgeOrganization(context.Background(), "org-a"); err == nil {
		t.Fatal("eraser without a provider error = nil, want not configured")
	}
	if _, err := nilEraser.PurgeUser(context.Background(), "user-1"); err == nil {
		t.Fatal("nil eraser PurgeUser() error = nil, want not configured")
	}
	if _, err := NewProviderEraser(nil, newFakeProviderIdentities()).PurgeUser(context.Background(), "user-1"); err == nil {
		t.Fatal("eraser without a membership store PurgeUser() error = nil, want not configured")
	}
}

// A user's erasure removes the messages and then the subscriber of every membership the user holds, in every organization,
// active and removed alike. Messages go first, so the listing and the provider both still find them.
func TestProviderErasureForUserDeletesEveryMembershipAcrossOrganizations(t *testing.T) {
	lister := &fakeMembershipLister{memberships: []Membership{
		membershipFor("org-a", "user-1", MembershipStatusActive),
		membershipFor("org-b", "user-1", MembershipStatusRemoved),
	}}
	provider := newFakeProviderIdentities()
	provider.messages[providerSubscriberID("org-a", "user-1")] = 3
	provider.messages[providerSubscriberID("org-b", "user-1")] = 1
	eraser := NewProviderEraser(lister, provider)

	counts, err := eraser.PurgeUser(context.Background(), "user-1")
	if err != nil {
		t.Fatalf("PurgeUser() error = %v", err)
	}
	if len(lister.userIDs) != 1 || lister.userIDs[0] != "user-1" || len(lister.orgIDs) != 0 {
		t.Fatalf("lister asked about users %v and organizations %v, want exactly user-1", lister.userIDs, lister.orgIDs)
	}
	first, second := providerSubscriberID("org-a", "user-1"), providerSubscriberID("org-b", "user-1")
	want := []string{"messages:" + first, "subscriber:" + first, "messages:" + second, "subscriber:" + second}
	if strings.Join(provider.calls, "|") != strings.Join(want, "|") {
		t.Fatalf("provider calls = %v, want %v", provider.calls, want)
	}
	if counts["novu_subscriber_messages"] != 4 || counts["novu_subscribers"] != 2 {
		t.Fatalf("counts = %v, want 4 messages and 2 subscribers", counts)
	}
}

// The lister is handed another user's row, as a faulty store might return it. The eraser must refuse it: that user's
// subscriber is not this user's to erase.
func TestProviderErasureForUserNeverTouchesAnotherUser(t *testing.T) {
	lister := &fakeMembershipLister{memberships: []Membership{
		membershipFor("org-a", "user-1", MembershipStatusActive),
		membershipFor("org-a", "user-2", MembershipStatusActive),
	}}
	provider := newFakeProviderIdentities()
	eraser := NewProviderEraser(lister, provider)

	counts, err := eraser.PurgeUser(context.Background(), "user-1")
	if err != nil {
		t.Fatalf("PurgeUser() error = %v", err)
	}
	other := providerSubscriberID("org-a", "user-2")
	for _, call := range provider.calls {
		if strings.Contains(call, other) {
			t.Fatalf("provider call %q reached another user's subscriber", call)
		}
	}
	if counts["novu_subscribers"] != 1 {
		t.Fatalf("counts = %v, want exactly one subscriber removed", counts)
	}
}

// An empty user id is refused before anything is listed or deleted, so it can never widen into a sweep of every user.
func TestProviderErasureForUserRefusesAnEmptyUser(t *testing.T) {
	lister := &fakeMembershipLister{memberships: []Membership{membershipFor("org-a", "user-1", MembershipStatusActive)}}
	provider := newFakeProviderIdentities()
	eraser := NewProviderEraser(lister, provider)

	if _, err := eraser.PurgeUser(context.Background(), "   "); !errors.Is(err, errProviderEraseUserRequired) {
		t.Fatalf("PurgeUser(empty) error = %v, want the user-required refusal", err)
	}
	if len(lister.userIDs) != 0 || len(provider.calls) != 0 {
		t.Fatalf("empty user listed %v and called %v, want nothing", lister.userIDs, provider.calls)
	}
}

// A replayed user erasure, or one whose subscribers were already removed, removes nothing and is not an error.
func TestProviderErasureForUserTreatsAnAlreadyRemovedSubscriberAsDone(t *testing.T) {
	lister := &fakeMembershipLister{memberships: []Membership{membershipFor("org-a", "user-1", MembershipStatusActive)}}
	provider := newFakeProviderIdentities()
	eraser := NewProviderEraser(lister, provider)

	if _, err := eraser.PurgeUser(context.Background(), "user-1"); err != nil {
		t.Fatalf("first PurgeUser() error = %v", err)
	}
	counts, err := eraser.PurgeUser(context.Background(), "user-1")
	if err != nil {
		t.Fatalf("replayed PurgeUser() error = %v, want nil for an already-removed subscriber", err)
	}
	if counts["novu_subscribers"] != 0 || counts["novu_subscriber_messages"] != 0 {
		t.Fatalf("replayed counts = %v, want zero", counts)
	}
}

// A message that cannot be deleted stops the user erasure before the subscriber is touched, so the retry starts again from the
// messages.
func TestProviderErasureForUserStopsBeforeTheSubscriberWhenMessagesFail(t *testing.T) {
	subscriber := providerSubscriberID("org-a", "user-1")
	lister := &fakeMembershipLister{memberships: []Membership{membershipFor("org-a", "user-1", MembershipStatusActive)}}
	provider := newFakeProviderIdentities()
	provider.messagesErr[subscriber] = errors.New("provider refused the message delete")
	eraser := NewProviderEraser(lister, provider)

	if _, err := eraser.PurgeUser(context.Background(), "user-1"); err == nil {
		t.Fatal("PurgeUser() error = nil, want the message failure")
	}
	for _, call := range provider.calls {
		if strings.HasPrefix(call, "subscriber:") {
			t.Fatalf("subscriber %q was deleted after its messages failed: calls = %v", subscriber, provider.calls)
		}
	}
}

// A failed listing of the user's memberships is returned before any provider call.
func TestProviderErasureForUserListFailureIsReturnedBeforeAnyProviderCall(t *testing.T) {
	lister := &fakeMembershipLister{err: errors.New("database unavailable")}
	provider := newFakeProviderIdentities()
	eraser := NewProviderEraser(lister, provider)

	if _, err := eraser.PurgeUser(context.Background(), "user-1"); err == nil {
		t.Fatal("PurgeUser() error = nil, want the listing failure")
	}
	if len(provider.calls) != 0 {
		t.Fatalf("provider calls = %v after a listing failure, want none", provider.calls)
	}
}
