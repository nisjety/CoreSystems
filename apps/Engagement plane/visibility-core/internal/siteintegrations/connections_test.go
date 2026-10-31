package siteintegrations

import "testing"

func TestConnectionFingerprintIgnoresOrderButTracksAccountAndPermissions(t *testing.T) {
	a := []connection{{ID: "google-a", ProviderKey: "google", Status: "active", AccountID: "one", Scopes: []string{"b", "a"}}}
	b := []connection{{ID: "google-a", ProviderKey: "google", Status: "active", AccountID: "one", Scopes: []string{"a", "b"}}}
	if connectionFingerprint(a) != connectionFingerprint(b) {
		t.Fatal("scope order changed authority fingerprint")
	}
	b[0].AccountID = "two"
	if connectionFingerprint(a) == connectionFingerprint(b) {
		t.Fatal("account replacement was ignored")
	}
	b[0].AccountID = "one"
	b[0].Status = "revoked"
	if connectionFingerprint(a) == connectionFingerprint(b) {
		t.Fatal("revocation was ignored")
	}
}
