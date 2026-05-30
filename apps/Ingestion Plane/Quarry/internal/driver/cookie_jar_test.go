package driver

import (
	"net/http"
	"sync"
	"testing"
)

// ─── GetJar / domain sharing ────────────────────────────────────────────────

func TestCookieStore_GetJar_SubdomainsShareJar(t *testing.T) {
	cs := NewCookieStore()

	jar1 := cs.GetJar("https://www.example.com/path")
	jar2 := cs.GetJar("https://api.example.com/v1/test")

	// Both subdomains must return the exact same jar instance (eTLD+1 = example.com).
	if jar1 != jar2 {
		t.Error("www.example.com and api.example.com should share the same cookie jar")
	}
}

func TestCookieStore_GetJar_DifferentDomainsHaveOwnJar(t *testing.T) {
	cs := NewCookieStore()

	jar1 := cs.GetJar("https://example.com")
	jar2 := cs.GetJar("https://another.com")

	if jar1 == jar2 {
		t.Error("example.com and another.com must NOT share a cookie jar")
	}
}

func TestCookieStore_GetJar_InvalidURL_Fallback(t *testing.T) {
	cs := NewCookieStore()
	// Must not panic on invalid URL.
	jar := cs.GetJar("not-a-url")
	if jar == nil {
		t.Error("GetJar with invalid URL must return a non-nil fallback jar")
	}
}

// ─── SetCookies / Cookies ───────────────────────────────────────────────────

func TestCookieStore_SetAndGet_Cookies(t *testing.T) {
	cs := NewCookieStore()
	target := "https://example.com"

	cs.SetCookies(target, []*http.Cookie{
		{Name: "session", Value: "abc123", Path: "/"},
		{Name: "user_id", Value: "42", Path: "/"},
	})

	cookies := cs.Cookies(target)
	found := map[string]string{}
	for _, c := range cookies {
		found[c.Name] = c.Value
	}

	if found["session"] != "abc123" {
		t.Errorf("expected session=abc123, got %q", found["session"])
	}
	if found["user_id"] != "42" {
		t.Errorf("expected user_id=42, got %q", found["user_id"])
	}
}

func TestCookieStore_CookiesAvailableAcrossSubdomains(t *testing.T) {
	cs := NewCookieStore()

	// Set on www subdomain.
	cs.SetCookies("https://www.example.com", []*http.Cookie{
		{Name: "auth", Value: "token-xyz", Path: "/", Domain: ".example.com"},
	})

	// Retrieve via the same subdomain — due to same jar the cookie should be there.
	cookies := cs.Cookies("https://www.example.com")
	found := false
	for _, c := range cookies {
		if c.Name == "auth" && c.Value == "token-xyz" {
			found = true
		}
	}
	if !found {
		t.Error("cookie set on www.example.com should be retrievable from the same domain")
	}
}

// ─── ClearDomain ────────────────────────────────────────────────────────────

func TestCookieStore_ClearDomain_RemovesCookies(t *testing.T) {
	cs := NewCookieStore()
	target := "https://example.com"

	cs.SetCookies(target, []*http.Cookie{{Name: "tok", Value: "val", Path: "/"}})
	cs.ClearDomain(target)

	// After clear the jar for this domain is removed — GetJar creates a fresh empty one.
	cookies := cs.Cookies(target)
	for _, c := range cookies {
		if c.Name == "tok" {
			t.Error("cookie should have been removed by ClearDomain")
		}
	}
}

func TestCookieStore_ClearDomain_InvalidURL_NoPanic(t *testing.T) {
	cs := NewCookieStore()
	defer func() {
		if r := recover(); r != nil {
			t.Errorf("ClearDomain with invalid URL panicked: %v", r)
		}
	}()
	cs.ClearDomain("not-a-url")
}

// ─── Thread safety ──────────────────────────────────────────────────────────

func TestCookieStore_ConcurrentAccess_NoPanic(t *testing.T) {
	cs := NewCookieStore()
	urls := []string{
		"https://alpha.example.com",
		"https://beta.example.com",
		"https://other.io",
		"https://third.net/path",
	}

	var wg sync.WaitGroup
	for i := 0; i < 50; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			u := urls[i%len(urls)]
			cs.SetCookies(u, []*http.Cookie{{Name: "n", Value: "v", Path: "/"}})
			_ = cs.Cookies(u)
			if i%7 == 0 {
				cs.ClearDomain(u)
			}
		}(i)
	}
	wg.Wait()
}
