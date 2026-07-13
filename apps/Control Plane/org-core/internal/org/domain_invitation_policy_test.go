package org

import "testing"

func TestDomainInvitationEligibility(t *testing.T) {
	tests := []struct {
		name          string
		email         string
		emailVerified bool
		domain        OrganizationDomain
		want          bool
	}{
		{
			name:          "verified exact company domain",
			email:         "Ima.DaCosta@Aquatiq.com",
			emailVerified: true,
			domain:        OrganizationDomain{NormalizedDomain: "aquatiq.com", Status: "verified", AutoInviteEnabled: true},
			want:          true,
		},
		{
			name:          "unverified user email",
			email:         "ima.dacosta@aquatiq.com",
			emailVerified: false,
			domain:        OrganizationDomain{NormalizedDomain: "aquatiq.com", Status: "verified", AutoInviteEnabled: true},
		},
		{
			name:          "pending domain proof",
			email:         "ima.dacosta@aquatiq.com",
			emailVerified: true,
			domain:        OrganizationDomain{NormalizedDomain: "aquatiq.com", Status: "pending", AutoInviteEnabled: true},
		},
		{
			name:          "public mail domain",
			email:         "ima@gmail.com",
			emailVerified: true,
			domain:        OrganizationDomain{NormalizedDomain: "gmail.com", Status: "verified", AutoInviteEnabled: true},
		},
		{
			name:          "apple relay",
			email:         "random@privaterelay.appleid.com",
			emailVerified: true,
			domain:        OrganizationDomain{NormalizedDomain: "privaterelay.appleid.com", Status: "verified", AutoInviteEnabled: true},
		},
		{
			name:          "different domain",
			email:         "ima@other.example",
			emailVerified: true,
			domain:        OrganizationDomain{NormalizedDomain: "aquatiq.com", Status: "verified", AutoInviteEnabled: true},
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := DomainInvitationEligible(test.email, test.emailVerified, test.domain); got != test.want {
				t.Fatalf("DomainInvitationEligible() = %v, want %v", got, test.want)
			}
		})
	}
}
