package org

import "strings"

var blockedAutoInviteDomains = map[string]struct{}{
	"gmail.com":                {},
	"googlemail.com":           {},
	"hotmail.com":              {},
	"outlook.com":              {},
	"live.com":                 {},
	"icloud.com":               {},
	"me.com":                   {},
	"yahoo.com":                {},
	"proton.me":                {},
	"protonmail.com":           {},
	"privaterelay.appleid.com": {},
}

func DomainInvitationEligible(email string, emailVerified bool, domain OrganizationDomain) bool {
	if !emailVerified || domain.Status != "verified" || !domain.AutoInviteEnabled {
		return false
	}
	parts := strings.Split(strings.ToLower(strings.TrimSpace(email)), "@")
	if len(parts) != 2 || parts[0] == "" || parts[1] == "" {
		return false
	}
	emailDomain := strings.TrimSuffix(parts[1], ".")
	normalizedDomain := strings.TrimSuffix(strings.ToLower(strings.TrimSpace(domain.NormalizedDomain)), ".")
	if _, blocked := blockedAutoInviteDomains[emailDomain]; blocked {
		return false
	}
	return emailDomain == normalizedDomain
}
