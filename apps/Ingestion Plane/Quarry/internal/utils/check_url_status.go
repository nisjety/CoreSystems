package utils

import (
	"fmt"
	"net/http"
	"time"
)

// checkURLStatus makes a HEAD request to check the status code of a URL
func CheckURLStatus(url string) int {
	client := &http.Client{
		Timeout: 10 * time.Second,
		// Follow redirects but limit them
		CheckRedirect: func(req *http.Request, via []*http.Request) error {
			if len(via) >= 10 {
				return fmt.Errorf("too many redirects")
			}
			return nil
		},
	}

	// Use HEAD request to avoid downloading the entire page
	req, err := http.NewRequest("HEAD", url, nil)
	if err != nil {
		return 0
	}

	// Set a reasonable User-Agent
	req.Header.Set("User-Agent", "Mozilla/5.0 (compatible; LinkChecker/1.0)")

	resp, err := client.Do(req)
	if err != nil {
		// If HEAD request fails, try GET request as fallback
		// Some servers don't support HEAD requests
		req, err := http.NewRequest("GET", url, nil)
		if err != nil {
			return 0
		}
		req.Header.Set("User-Agent", "Mozilla/5.0 (compatible; LinkChecker/1.0)")

		resp, err := client.Do(req)
		if err != nil {
			return 0
		}
		defer resp.Body.Close()
		return resp.StatusCode
	}
	defer resp.Body.Close()

	return resp.StatusCode
}
