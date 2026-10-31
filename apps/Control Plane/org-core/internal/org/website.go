package org

import (
	"errors"
	"net"
	"net/url"
	"strings"
)

// ErrInvalidWebsite means the value is not a web address the organisation's record can keep.
var ErrInvalidWebsite = errors.New("org: website is not a web address")

// maxWebsiteLength bounds the stored address. A company's home page is far shorter.
const maxWebsiteLength = 2048

// NormalizeWebsite returns the website as the record keeps it: an http or https address with a host name,
// without user information or a fragment, and with the scheme and host in lower case. Anything else is
// refused, so the record never holds a value that is not a web address.
func NormalizeWebsite(raw string) (string, error) {
	s := strings.TrimSpace(raw)
	if s == "" || len(s) > maxWebsiteLength || strings.ContainsAny(s, " \t\r\n") {
		return "", ErrInvalidWebsite
	}
	u, err := url.Parse(s)
	if err != nil {
		return "", ErrInvalidWebsite
	}
	scheme := strings.ToLower(u.Scheme)
	if scheme != "http" && scheme != "https" {
		return "", ErrInvalidWebsite
	}
	host := strings.ToLower(u.Hostname())
	if host == "" || u.User != nil || u.Fragment != "" || net.ParseIP(host) != nil ||
		!strings.Contains(host, ".") || strings.HasPrefix(host, ".") || strings.HasSuffix(host, ".") {
		return "", ErrInvalidWebsite
	}
	u.Scheme = scheme
	u.Host = strings.ToLower(u.Host)
	return u.String(), nil
}
