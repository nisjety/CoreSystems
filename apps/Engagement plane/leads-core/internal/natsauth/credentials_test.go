package natsauth

import "testing"

func TestSelectPrefersScopedPrincipalAndGatesTokenFallback(t *testing.T) {
	got, err := Select("application-leads", "0123456789abcdef0123456789abcdef")
	if err != nil || got.User != "application-leads" {
		t.Fatalf("scoped = %+v, %v", got, err)
	}
	if _, err := Select("", "abcdef0123456789abcdef0123456789"); err == nil {
		t.Fatal("missing scoped user was accepted")
	}
}
