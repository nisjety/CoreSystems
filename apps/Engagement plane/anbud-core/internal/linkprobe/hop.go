package linkprobe

import (
	"context"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"regexp"
	"strings"
)

// PageBytes is how much of a page the probe reads to find its links. It is far more than the head
// of a document needs, and it is bounded, so a large file cannot be pulled in whole.
const PageBytes = 512 << 10

// FetchPage asks for a page once and reads up to PageBytes of it, following at most three
// redirects, with no cookies. It returns the URL the page came from after redirects, so that its
// relative links resolve against where it really is. An answer that is not a success is an error.
func FetchPage(ctx context.Context, client *http.Client, link string) ([]byte, *url.URL, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, link, nil)
	if err != nil {
		return nil, nil, err
	}
	req.Header.Set("User-Agent", UserAgent)
	req.Header.Set("Accept", "text/html,*/*;q=0.5")
	resp, err := client.Do(req)
	if err != nil {
		return nil, nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return nil, nil, fmt.Errorf("the page answered with status %d", resp.StatusCode)
	}
	body, err := io.ReadAll(io.LimitReader(resp.Body, PageBytes))
	if err != nil {
		return nil, nil, err
	}
	final := resp.Request.URL
	return body, final, nil
}

var (
	hrefLink = regexp.MustCompile(`(?i)href\s*=\s*["']([^"'#][^"']*)["']`)

	documentExtensions = []string{".pdf", ".zip", ".doc", ".docx", ".xls", ".xlsx", ".odt", ".rtf"}
	documentWords      = []string{"download", "vedlegg", "dokument"}
)

// DocumentLinks returns the links of a page that point at a document, resolved against the page's
// own URL, in order and each once. Only links on the same host as the page are returned: that is
// the one hop the probe makes. The number of document links on other hosts is returned too, and
// not followed. A link to a page, to mail or to a script is not a document and is not counted.
func DocumentLinks(page []byte, base *url.URL) (same []string, otherHost int) {
	seen := map[string]bool{}
	for _, m := range hrefLink.FindAllSubmatch(page, -1) {
		ref, err := url.Parse(strings.TrimSpace(string(m[1])))
		if err != nil {
			continue
		}
		abs := base.ResolveReference(ref)
		if abs.Scheme != "http" && abs.Scheme != "https" {
			continue
		}
		if !looksLikeDocument(abs) {
			continue
		}
		if abs.Host != base.Host {
			otherHost++
			continue
		}
		key := abs.String()
		if seen[key] {
			continue
		}
		seen[key] = true
		same = append(same, key)
	}
	return same, otherHost
}

// looksLikeDocument says a URL names a document: by its extension, or by a word a download link
// carries in Norwegian and English.
func looksLikeDocument(u *url.URL) bool {
	path := strings.ToLower(u.Path)
	for _, ext := range documentExtensions {
		if strings.HasSuffix(path, ext) {
			return true
		}
	}
	rest := strings.ToLower(u.Path + "?" + u.RawQuery)
	for _, word := range documentWords {
		if strings.Contains(rest, word) {
			return true
		}
	}
	return false
}
