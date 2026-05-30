package driver

import (
	"net/http"
	"net/http/cookiejar"
	"net/url"
	"sync"

	"golang.org/x/net/publicsuffix"
)

// CookieStore provides a thread-safe cookie jar that persists cookies across
// requests to the same domain. This is critical for sites that set cookies on
// the challenge page and require them on subsequent requests (Cloudflare,
// DataDome, etc.).
type CookieStore struct {
	mu   sync.RWMutex
	jars map[string]*cookiejar.Jar // keyed by eTLD+1
}

// NewCookieStore creates a new empty cookie store.
func NewCookieStore() *CookieStore {
	return &CookieStore{
		jars: make(map[string]*cookiejar.Jar),
	}
}

// GetJar returns (or creates) the cookie jar for the given URL's domain.
func (cs *CookieStore) GetJar(targetURL string) http.CookieJar {
	u, err := url.Parse(targetURL)
	if err != nil {
		return cs.defaultJar()
	}
	domain := u.Hostname()
	// Use effective TLD+1 so subdomains share cookies (e.g. www.example.com
	// and cdn.example.com share the example.com jar).
	etld1, err := publicsuffix.EffectiveTLDPlusOne(domain)
	if err != nil {
		etld1 = domain
	}

	cs.mu.RLock()
	jar, ok := cs.jars[etld1]
	cs.mu.RUnlock()
	if ok {
		return jar
	}

	cs.mu.Lock()
	defer cs.mu.Unlock()
	// Double-check under write lock.
	if jar, ok = cs.jars[etld1]; ok {
		return jar
	}
	jar, _ = cookiejar.New(&cookiejar.Options{PublicSuffixList: publicsuffix.List})
	cs.jars[etld1] = jar
	return jar
}

// SetCookies adds cookies for a URL into the appropriate jar.
func (cs *CookieStore) SetCookies(targetURL string, cookies []*http.Cookie) {
	u, err := url.Parse(targetURL)
	if err != nil {
		return
	}
	jar := cs.GetJar(targetURL)
	jar.SetCookies(u, cookies)
}

// Cookies returns cookies stored for the given URL.
func (cs *CookieStore) Cookies(targetURL string) []*http.Cookie {
	u, err := url.Parse(targetURL)
	if err != nil {
		return nil
	}
	jar := cs.GetJar(targetURL)
	return jar.Cookies(u)
}

// ClearDomain removes all cookies for a domain.
func (cs *CookieStore) ClearDomain(targetURL string) {
	u, err := url.Parse(targetURL)
	if err != nil {
		return
	}
	domain := u.Hostname()
	etld1, err := publicsuffix.EffectiveTLDPlusOne(domain)
	if err != nil {
		etld1 = domain
	}
	cs.mu.Lock()
	delete(cs.jars, etld1)
	cs.mu.Unlock()
}

func (cs *CookieStore) defaultJar() http.CookieJar {
	jar, _ := cookiejar.New(&cookiejar.Options{PublicSuffixList: publicsuffix.List})
	return jar
}
