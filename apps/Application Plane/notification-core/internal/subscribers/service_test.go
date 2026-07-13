package subscribers

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/delegation"
)

type fakeRepository struct {
	activeByScope      map[string]*Subscriber
	membershipsByUser  map[string][]Membership
	upsertedMembership MembershipParams
	membershipResult   *Membership
	membershipErr      error
}

func TestResolveUserDoesNotTreatGatewaySessionScopeAsMembershipAuthority(t *testing.T) {
	service := NewService(&fakeRepository{}, nil)
	ctx := delegation.WithPrincipal(context.Background(), delegation.Principal{
		ServiceID:      "velion-gateway",
		UserID:         "user-1",
		OrganizationID: "org-a",
		Role:           "member",
	})

	providerID, err := service.ResolveUser(ctx, "org-a", "user-1")
	if !errors.Is(err, ErrNotFound) || providerID != "" {
		t.Fatalf("matching session scope ResolveUser() = (%q, %v), want no membership authority", providerID, err)
	}
	if providerID, err = service.ResolveUser(ctx, "org-b", "user-1"); !errors.Is(err, ErrNotFound) || providerID != "" {
		t.Fatalf("foreign-org ResolveUser() = (%q, %v), want not found", providerID, err)
	}
	if providerID, err = service.ResolveUser(ctx, "org-a", "user-2"); !errors.Is(err, ErrNotFound) || providerID != "" {
		t.Fatalf("foreign-user ResolveUser() = (%q, %v), want not found", providerID, err)
	}
}

func (f *fakeRepository) Upsert(context.Context, UpsertParams) (*Subscriber, error) {
	return nil, errors.New("not implemented")
}

func (f *fakeRepository) Get(context.Context, string) (*Subscriber, error) {
	return nil, ErrNotFound
}

func (f *fakeRepository) GetActiveForOrganization(_ context.Context, organizationID, userID string) (*Subscriber, error) {
	subscriber := f.activeByScope[organizationID+"\x00"+userID]
	if subscriber == nil {
		return nil, ErrNotFound
	}
	copy := *subscriber
	return &copy, nil
}

func (f *fakeRepository) UpsertMembership(_ context.Context, params MembershipParams) (*Membership, error) {
	f.upsertedMembership = params
	if f.membershipErr != nil {
		return nil, f.membershipErr
	}
	if f.membershipResult != nil {
		copy := *f.membershipResult
		copy.ProviderSubscriberID = params.ProviderSubscriberID
		return &copy, nil
	}
	return &Membership{
		OrganizationID:       params.OrganizationID,
		UserID:               params.UserID,
		ProviderSubscriberID: params.ProviderSubscriberID,
		Status:               params.Status,
		OccurredAt:           params.OccurredAt,
	}, nil
}

func (f *fakeRepository) ListActiveMembershipsForUser(_ context.Context, userID string) ([]Membership, error) {
	return f.membershipsByUser[userID], nil
}

func TestResolveUserRequiresActiveOrganizationMembership(t *testing.T) {
	repository := &fakeRepository{activeByScope: map[string]*Subscriber{
		"org-a\x00user-1": {UserID: "user-1", NovuSubscriberID: "provider-a"},
	}}
	service := NewService(repository, nil)

	providerID, err := service.ResolveUser(context.Background(), "org-a", "user-1")
	if err != nil || providerID != "provider-a" {
		t.Fatalf("ResolveUser(org-a) = (%q, %v), want provider-a", providerID, err)
	}
	if providerID, err = service.ResolveUser(context.Background(), "org-b", "user-1"); !errors.Is(err, ErrNotFound) || providerID != "" {
		t.Fatalf("ResolveUser(org-b) = (%q, %v), want not found", providerID, err)
	}
}

func TestProviderSubscriberIDIsStableAndTenantIsolated(t *testing.T) {
	first := providerSubscriberID("org-a", "user-1")
	if first != providerSubscriberID("org-a", "user-1") {
		t.Fatal("provider subscriber id is not stable")
	}
	if first == providerSubscriberID("org-b", "user-1") {
		t.Fatal("provider subscriber id is shared across organizations")
	}
	if first == providerSubscriberID("org-a", "user-2") {
		t.Fatal("provider subscriber id is shared across users")
	}
}

func TestUpsertMembershipDerivesProviderIdentityAndPreservesRemoval(t *testing.T) {
	repository := &fakeRepository{}
	service := NewService(repository, nil)
	occurredAt := time.Date(2026, time.July, 13, 10, 0, 0, 0, time.UTC)
	revision := int64(9)

	membership, err := service.UpsertMembership(context.Background(), MembershipParams{
		OrganizationID:       "org-a",
		UserID:               "user-1",
		ProviderSubscriberID: "caller-controlled",
		Role:                 "member",
		Status:               MembershipStatusRemoved,
		AuthorityRevision:    &revision,
		SourceEventID:        "event-9",
		OccurredAt:           occurredAt,
	})
	if err != nil {
		t.Fatalf("UpsertMembership() error = %v", err)
	}
	wantProviderID := providerSubscriberID("org-a", "user-1")
	if repository.upsertedMembership.ProviderSubscriberID != wantProviderID {
		t.Fatalf("stored provider id = %q, want derived %q", repository.upsertedMembership.ProviderSubscriberID, wantProviderID)
	}
	if membership.Status != MembershipStatusRemoved {
		t.Fatalf("membership status = %q, want removed", membership.Status)
	}
}
