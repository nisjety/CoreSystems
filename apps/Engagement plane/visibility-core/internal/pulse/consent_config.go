package pulse

import (
	"errors"
	"net/url"
	"path"
	"strconv"
	"strings"
)

const (
	DefaultGoogleConsentReturnURL = "http://localhost:5173/studio/pulse"
	DefaultGoogleOAuthCallbackURL = "http://localhost:3026/oauth/callback/google"
)

// ResolveGoogleConsentURLs validates deployment-owned exact destinations. These
// values are never derived from a request, the website being analyzed, or a
// forwarded Host header. Integration Core must separately allow the return URL
// and register the callback with Google; this does not bypass those controls.
func ResolveGoogleConsentURLs(returnURL, callbackURL string) (string, string, error) {
	if returnURL == "" {
		returnURL = DefaultGoogleConsentReturnURL
	}
	if callbackURL == "" {
		callbackURL = DefaultGoogleOAuthCallbackURL
	}
	if !validConsentDestination(returnURL) || !validConsentDestination(callbackURL) {
		return "", "", errors.New("Google consent destinations must be canonical HTTPS URLs or explicit local loopback HTTP URLs")
	}
	return returnURL, callbackURL, nil
}

func validConsentDestination(raw string) bool {
	if len(raw) > 2048 || strings.ContainsAny(raw, "?#%\\\r\n\t ") {
		return false
	}
	parsed, err := url.Parse(raw)
	if err != nil || parsed.User != nil || parsed.Opaque != "" || parsed.RawPath != "" || parsed.RawQuery != "" || parsed.Fragment != "" || parsed.ForceQuery || parsed.String() != raw {
		return false
	}
	if parsed.Path == "" || !strings.HasPrefix(parsed.Path, "/") || path.Clean(parsed.Path) != parsed.Path || strings.Contains(parsed.Path, "//") {
		return false
	}
	if parsed.Scheme == "https" {
		return parsed.Host == parsed.Hostname() && parsed.Host == strings.ToLower(parsed.Host) && validPublicHostname(parsed.Host)
	}
	if parsed.Scheme != "http" || !(parsed.Hostname() == "localhost" || parsed.Hostname() == "127.0.0.1" || parsed.Hostname() == "::1") {
		return false
	}
	port := parsed.Port()
	if port != "" {
		number, err := strconv.Atoi(port)
		if err != nil || number < 1 || number > 65535 || strconv.Itoa(number) != port {
			return false
		}
	}
	host := parsed.Hostname()
	if host == "::1" {
		host = "[::1]"
	}
	if port != "" {
		host += ":" + port
	}
	return parsed.Host == host
}
