package transform

import (
	"context"
	"net/url"
	"regexp"
	"strings"

	"github.com/PuerkitoBio/goquery"
	"github.com/rs/zerolog/log"
)

// ContentDoc holds the mutable document state flowing through transformer steps.
type ContentDoc struct {
	URL         string
	HTML        string
	ContentType string
	Metadata    map[string]string
}

// Transformer is a single composable transformation step.
type Transformer interface {
	Name() string
	Transform(ctx context.Context, doc *ContentDoc) error
}

// ContentPipeline chains transformers in order.
type ContentPipeline struct {
	steps []Transformer
}

// NewContentPipeline creates a pipeline from the given transformers.
func NewContentPipeline(steps ...Transformer) *ContentPipeline {
	valid := make([]Transformer, 0, len(steps))
	for _, s := range steps {
		if s != nil {
			valid = append(valid, s)
		}
	}
	return &ContentPipeline{steps: valid}
}

// Run executes all transformer steps sequentially.
func (p *ContentPipeline) Run(ctx context.Context, doc *ContentDoc) error {
	if p == nil || doc == nil {
		return nil
	}
	if doc.Metadata == nil {
		doc.Metadata = map[string]string{}
	}
	for _, step := range p.steps {
		if err := ctx.Err(); err != nil {
			return err
		}
		if err := step.Transform(ctx, doc); err != nil {
			log.Warn().Err(err).Str("step", step.Name()).Msg("transformer step failed, continuing")
			// Non-fatal: continue pipeline on individual step failure.
		}
	}
	return nil
}

// DefaultContentPipeline returns the standard pre-processing chain.
func DefaultContentPipeline() *ContentPipeline {
	return NewContentPipeline(
		&ScriptStripper{},
		&Base64ImageStripper{},
		&LinkResolver{},
		&LazyImageResolver{},
		&TableNormalizer{},
		&CanonicalURLNormalizer{},
		&DublinCoreExtractor{},
	)
}

// ── ScriptStripper removes script, style, noscript, and tracking elements ──

type ScriptStripper struct{}

func (s *ScriptStripper) Name() string { return "script_stripper" }

func (s *ScriptStripper) Transform(_ context.Context, doc *ContentDoc) error {
	d, err := goquery.NewDocumentFromReader(strings.NewReader(doc.HTML))
	if err != nil {
		return err
	}

	d.Find("script, noscript, iframe[src*='analytics'], iframe[src*='tracking']").Each(func(_ int, sel *goquery.Selection) {
		sel.Remove()
	})

	// Remove hidden elements that are typically tracking pixels.
	d.Find(`img[width="1"][height="1"], img[style*="display:none"], img[style*="display: none"]`).Each(func(_ int, sel *goquery.Selection) {
		sel.Remove()
	})

	html, err := d.Find("body").Html()
	if err != nil {
		return err
	}
	doc.HTML = html
	return nil
}

// ── LinkResolver converts relative URLs to absolute ──────────────────────

type LinkResolver struct{}

func (l *LinkResolver) Name() string { return "link_resolver" }

func (l *LinkResolver) Transform(_ context.Context, doc *ContentDoc) error {
	baseURL, err := url.Parse(doc.URL)
	if err != nil {
		return nil // Skip silently on invalid base URL.
	}

	d, err := goquery.NewDocumentFromReader(strings.NewReader(doc.HTML))
	if err != nil {
		return err
	}

	resolveAttr := func(sel *goquery.Selection, attr string) {
		if val, exists := sel.Attr(attr); exists {
			if parsed, err := url.Parse(val); err == nil && !parsed.IsAbs() {
				sel.SetAttr(attr, baseURL.ResolveReference(parsed).String())
			}
		}
	}

	d.Find("a[href]").Each(func(_ int, sel *goquery.Selection) {
		resolveAttr(sel, "href")
	})
	d.Find("img[src]").Each(func(_ int, sel *goquery.Selection) {
		resolveAttr(sel, "src")
	})
	d.Find("source[src], source[srcset]").Each(func(_ int, sel *goquery.Selection) {
		resolveAttr(sel, "src")
		// srcset has a special format: "url size, url size, ..."
		if srcset, exists := sel.Attr("srcset"); exists {
			sel.SetAttr("srcset", resolveSrcset(baseURL, srcset))
		}
	})

	html, err := d.Find("body").Html()
	if err != nil {
		return err
	}
	doc.HTML = html
	return nil
}

func resolveSrcset(base *url.URL, srcset string) string {
	parts := strings.Split(srcset, ",")
	resolved := make([]string, 0, len(parts))
	for _, part := range parts {
		fields := strings.Fields(strings.TrimSpace(part))
		if len(fields) == 0 {
			continue
		}
		if parsed, err := url.Parse(fields[0]); err == nil && !parsed.IsAbs() {
			fields[0] = base.ResolveReference(parsed).String()
		}
		resolved = append(resolved, strings.Join(fields, " "))
	}
	return strings.Join(resolved, ", ")
}

// ── LazyImageResolver converts data-src / data-lazy-src to src ──────────

type LazyImageResolver struct{}

func (l *LazyImageResolver) Name() string { return "lazy_image_resolver" }

var lazyAttrs = []string{"data-src", "data-lazy-src", "data-original", "data-srcset"}

func (l *LazyImageResolver) Transform(_ context.Context, doc *ContentDoc) error {
	d, err := goquery.NewDocumentFromReader(strings.NewReader(doc.HTML))
	if err != nil {
		return err
	}

	d.Find("img").Each(func(_ int, sel *goquery.Selection) {
		src, _ := sel.Attr("src")
		if src != "" && !isPlaceholder(src) {
			return // Already has a real src.
		}
		for _, attr := range lazyAttrs {
			if val, exists := sel.Attr(attr); exists && val != "" {
				sel.SetAttr("src", val)
				return
			}
		}
	})

	html, err := d.Find("body").Html()
	if err != nil {
		return err
	}
	doc.HTML = html
	return nil
}

var placeholderRe = regexp.MustCompile(`(?i)(data:image|placeholder|grey\.gif|blank\.gif|spacer\.gif|pixel\.gif|1x1)`)

func isPlaceholder(src string) bool {
	return placeholderRe.MatchString(src)
}

// ── TableNormalizer ensures tables have proper headers and structure ──────

type TableNormalizer struct{}

func (t *TableNormalizer) Name() string { return "table_normalizer" }

func (t *TableNormalizer) Transform(_ context.Context, doc *ContentDoc) error {
	d, err := goquery.NewDocumentFromReader(strings.NewReader(doc.HTML))
	if err != nil {
		return err
	}

	d.Find("table").Each(func(_ int, table *goquery.Selection) {
		// If a table has no <thead> but the first <tr> contains <th> elements,
		// wrap it in a <thead> for proper markdown conversion.
		if table.Find("thead").Length() == 0 {
			firstRow := table.Find("tr").First()
			if firstRow.Find("th").Length() > 0 {
				firstRow.WrapAllHtml("<thead></thead>")
			}
		}
	})

	html, err := d.Find("body").Html()
	if err != nil {
		return err
	}
	doc.HTML = html
	return nil
}

// ── Base64ImageStripper removes inline data-URI images from HTML ──────────
// Large base64 blobs bloat HTML sent to LLMs and markdown renderers without
// providing any text value.

type Base64ImageStripper struct{}

func (b *Base64ImageStripper) Name() string { return "base64_image_stripper" }

func (b *Base64ImageStripper) Transform(_ context.Context, doc *ContentDoc) error {
	d, err := goquery.NewDocumentFromReader(strings.NewReader(doc.HTML))
	if err != nil {
		return err
	}

	d.Find("img, source").Each(func(_ int, sel *goquery.Selection) {
		for _, attr := range []string{"src", "srcset", "data-src"} {
			if val, exists := sel.Attr(attr); exists && strings.HasPrefix(strings.TrimSpace(val), "data:") {
				sel.Remove()
				return
			}
		}
	})

	out, err := d.Find("body").Html()
	if err != nil {
		return err
	}
	doc.HTML = out
	return nil
}

// ── CanonicalURLNormalizer extracts the canonical URL into metadata ────────

type CanonicalURLNormalizer struct{}

func (c *CanonicalURLNormalizer) Name() string { return "canonical_url_normalizer" }

func (c *CanonicalURLNormalizer) Transform(_ context.Context, doc *ContentDoc) error {
	d, err := goquery.NewDocumentFromReader(strings.NewReader(doc.HTML))
	if err != nil {
		return err
	}

	canonical := strings.TrimSpace(d.Find(`link[rel="canonical"]`).AttrOr("href", ""))
	if canonical == "" {
		return nil
	}

	if base, parseErr := url.Parse(doc.URL); parseErr == nil {
		if ref, parseErr := base.Parse(canonical); parseErr == nil {
			canonical = ref.String()
		}
	}
	doc.Metadata["canonical_url"] = canonical
	return nil
}

// ── DublinCoreExtractor reads DC, article, and extended meta into Metadata ─

type DublinCoreExtractor struct{}

func (dc *DublinCoreExtractor) Name() string { return "dublin_core_extractor" }

func (dc *DublinCoreExtractor) Transform(_ context.Context, doc *ContentDoc) error {
	d, err := goquery.NewDocumentFromReader(strings.NewReader(doc.HTML))
	if err != nil {
		return err
	}

	// <meta name="..."> — Dublin Core and common metadata names
	d.Find("meta[name]").Each(func(_ int, sel *goquery.Selection) {
		name := strings.ToLower(strings.TrimSpace(sel.AttrOr("name", "")))
		content := strings.TrimSpace(sel.AttrOr("content", ""))
		if content == "" {
			return
		}
		switch name {
		case "dc.title", "dcterms.title":
			doc.Metadata["dc:title"] = content
		case "dc.date", "dcterms.date", "dc.date.created", "dc.date.issued":
			doc.Metadata["dc:date"] = content
		case "dc.creator", "dcterms.creator", "dc.author":
			doc.Metadata["dc:creator"] = content
		case "dc.description", "dcterms.description":
			doc.Metadata["dc:description"] = content
		case "dc.language", "dcterms.language":
			doc.Metadata["dc:language"] = content
		case "dc.publisher", "dcterms.publisher":
			doc.Metadata["dc:publisher"] = content
		case "dc.subject", "dcterms.subject":
			doc.Metadata["dc:subject"] = content
		case "article:published_time":
			doc.Metadata["article:published_time"] = content
		case "article:modified_time", "article:modified":
			doc.Metadata["article:modified_time"] = content
		case "keywords":
			doc.Metadata["keywords"] = content
		}
	})

	// <meta property="..."> — Open Graph extended
	d.Find("meta[property]").Each(func(_ int, sel *goquery.Selection) {
		prop := strings.ToLower(strings.TrimSpace(sel.AttrOr("property", "")))
		content := strings.TrimSpace(sel.AttrOr("content", ""))
		if content == "" {
			return
		}
		switch prop {
		case "og:site_name":
			doc.Metadata["og:site_name"] = content
		case "og:locale":
			doc.Metadata["og:locale"] = content
		case "article:published_time":
			doc.Metadata["article:published_time"] = content
		case "article:modified_time":
			doc.Metadata["article:modified_time"] = content
		}
	})

	return nil
}
