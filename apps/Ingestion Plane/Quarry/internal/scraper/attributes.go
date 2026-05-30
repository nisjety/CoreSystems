package scraper

import (
	"context"
	"strings"

	"github.com/PuerkitoBio/goquery"
)

// AttributeSelector defines a CSS selector and the attribute(s) to extract.
type AttributeSelector struct {
	// Name is the key used in the output map for this selector's results.
	Name string `json:"name"`
	// Selector is a CSS selector string (e.g., "h1", ".price", "#main p").
	Selector string `json:"selector"`
	// Attribute is the HTML attribute to extract. Use "" or "text" to extract
	// the text content. Use "html" to extract inner HTML.
	Attribute string `json:"attribute,omitempty"`
	// Multiple controls whether all matching elements are returned (true) or just
	// the first match (false, default).
	Multiple bool `json:"multiple,omitempty"`
}

// extractAttributes applies the given selectors to the HTML document and
// returns a map of name → extracted value(s).
func extractAttributes(_ context.Context, html string, selectors []AttributeSelector) map[string]interface{} {
	result := make(map[string]interface{}, len(selectors))
	if len(selectors) == 0 || strings.TrimSpace(html) == "" {
		return result
	}

	doc, err := goquery.NewDocumentFromReader(strings.NewReader(html))
	if err != nil {
		return result
	}

	for _, sel := range selectors {
		if strings.TrimSpace(sel.Name) == "" || strings.TrimSpace(sel.Selector) == "" {
			continue
		}

		attr := strings.ToLower(strings.TrimSpace(sel.Attribute))
		extract := func(s *goquery.Selection) string {
			switch attr {
			case "", "text":
				return strings.TrimSpace(s.Text())
			case "html":
				inner, err := s.Html()
				if err != nil {
					return ""
				}
				return strings.TrimSpace(inner)
			default:
				return strings.TrimSpace(s.AttrOr(attr, ""))
			}
		}

		matched := doc.Find(sel.Selector)
		if matched.Length() == 0 {
			result[sel.Name] = nil
			continue
		}

		if sel.Multiple {
			vals := make([]string, 0, matched.Length())
			matched.Each(func(_ int, s *goquery.Selection) {
				if v := extract(s); v != "" {
					vals = append(vals, v)
				}
			})
			result[sel.Name] = vals
		} else {
			result[sel.Name] = extract(matched.First())
		}
	}

	return result
}
