package utils

import (
	"fmt"
	"net/url"
	"strings"

	"github.com/go-rod/rod"
	structtypes "github.com/triodelab/quarry/internal/struct_types"
)

func ExtractLinkInfo(link *rod.Element, baseURL *url.URL) (structtypes.LinkInfo, error) {
	// Get link text
	text, err := link.Text()
	if err != nil {
		text = "[No text]"
	}

	// Get href attribute
	href, err := link.Attribute("href")
	if err != nil || href == nil {
		return structtypes.LinkInfo{}, fmt.Errorf("no href attribute: %v", err)
	}

	// Resolve relative URLs
	resolvedURL, err := url.Parse(*href)
	if err != nil {
		return structtypes.LinkInfo{}, fmt.Errorf("invalid URL: %v", err)
	}

	// Make absolute URL
	absoluteURL := baseURL.ResolveReference(resolvedURL).String()

	// Status code will be determined by making HTTP request
	statusCode := 0

	// Get description (from title attribute or parent element)
	description := ""
	if title, err := link.Attribute("title"); err == nil && title != nil {
		description = *title
	}

	// Determine if it's internal or external
	isInternal := resolvedURL.Host == "" || resolvedURL.Host == baseURL.Host

	return structtypes.LinkInfo{
		Text:        strings.TrimSpace(text),
		URL:         absoluteURL,
		Description: strings.TrimSpace(description),
		IsInternal:  isInternal,
		StatusCode:  statusCode,
	}, nil
}
