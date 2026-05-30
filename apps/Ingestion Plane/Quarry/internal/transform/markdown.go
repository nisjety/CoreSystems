package transform

import (
	"fmt"
	"net/url"
	"strings"

	"github.com/JohannesKaufmann/html-to-markdown/v2/converter"
	"github.com/JohannesKaufmann/html-to-markdown/v2/plugin/base"
	"github.com/JohannesKaufmann/html-to-markdown/v2/plugin/commonmark"
	"github.com/JohannesKaufmann/html-to-markdown/v2/plugin/strikethrough"
	"github.com/JohannesKaufmann/html-to-markdown/v2/plugin/table"
	"github.com/go-shiori/go-readability"
)

func HTMLToMarkdown(pageURL, html string, onlyMainContent bool) (string, error) {
	sourceHTML := html
	if onlyMainContent {
		parsedURL, err := url.Parse(pageURL)
		if err != nil {
			return "", fmt.Errorf("parse url for readability: %w", err)
		}
		article, err := readability.FromReader(strings.NewReader(html), parsedURL)
		if err != nil {
			return "", fmt.Errorf("readability extraction failed: %w", err)
		}
		// Fall back to full HTML if readability extracted too little content.
		if len(strings.TrimSpace(article.Content)) > readabilityMinLen {
			sourceHTML = article.Content
		}
	}

	conv := converter.NewConverter(
		converter.WithPlugins(
			base.NewBasePlugin(),
			commonmark.NewCommonmarkPlugin(),
			strikethrough.NewStrikethroughPlugin(),
			table.NewTablePlugin(),
		),
	)

	markdown, err := conv.ConvertString(sourceHTML)
	if err != nil {
		return "", fmt.Errorf("html to markdown conversion failed: %w", err)
	}
	return strings.TrimSpace(markdown), nil
}

// readabilityMinLen is the minimum length of extracted content (in bytes)
// below which we fall back to the full HTML instead of the readability output.
const readabilityMinLen = 100
