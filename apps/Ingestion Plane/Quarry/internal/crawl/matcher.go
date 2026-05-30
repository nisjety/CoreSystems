package crawl

import (
	"fmt"
	"net/url"
	"strings"

	"github.com/gobwas/glob"
)

type compiledPattern interface {
	Match(target string) bool
}

type regexPattern struct {
	re PatternRegexp
}

func (p regexPattern) Match(target string) bool {
	return p.re.MatchString(target)
}

type globPattern struct {
	glob glob.Glob
	raw  string
}

func (p globPattern) Match(target string) bool {
	if p.glob.Match(target) {
		return true
	}
	if strings.HasSuffix(p.raw, "/**") && target == strings.TrimSuffix(p.raw, "/**") {
		return true
	}
	return false
}

type PatternRegexp interface {
	MatchString(string) bool
}

type Matcher struct {
	spec            Spec
	startURL        *url.URL
	startBaseDomain string
	startPath       string
	include         []compiledPattern
	exclude         []compiledPattern
	matchFullURL    bool
}

func NewMatcher(spec Spec) (*Matcher, error) {
	startURL, err := url.Parse(spec.URL)
	if err != nil {
		return nil, fmt.Errorf("parse start url: %w", err)
	}

	include, err := compilePatterns(spec.IncludePaths, spec.RegexOnFullURL || spec.RegexPaths)
	if err != nil {
		return nil, err
	}
	exclude, err := compilePatterns(spec.ExcludePaths, spec.RegexOnFullURL || spec.RegexPaths)
	if err != nil {
		return nil, err
	}

	return &Matcher{
		spec:            spec,
		startURL:        startURL,
		startBaseDomain: BaseDomain(startURL.Hostname()),
		startPath:       normalizePath(startURL.Path),
		include:         include,
		exclude:         exclude,
		matchFullURL:    spec.RegexOnFullURL,
	}, nil
}

// staticAssetExts lists file extensions that are never useful knowledge-base content.
// These are filtered out during link discovery to keep crawl results clean.
var staticAssetExts = map[string]struct{}{
	// Images
	".jpg": {}, ".jpeg": {}, ".png": {}, ".gif": {}, ".webp": {},
	".bmp": {}, ".tiff": {}, ".ico": {}, ".svg": {},
	// Web fonts
	".woff": {}, ".woff2": {}, ".ttf": {}, ".eot": {}, ".otf": {},
	// Stylesheets & scripts
	".css": {}, ".js": {}, ".mjs": {}, ".cjs": {},
	// Media
	".mp4": {}, ".mp3": {}, ".webm": {}, ".ogg": {}, ".wav": {},
	".avi": {}, ".mov": {}, ".flv": {},
	// Archives / binary
	".zip": {}, ".tar": {}, ".gz": {}, ".br": {},
	// Manifest / config
	".webmanifest": {}, ".map": {},
}

// isStaticAsset returns true when the URL path ends with a known static file
// extension that should never be indexed as a knowledge-base document.
func isStaticAsset(u *url.URL) bool {
	path := strings.ToLower(u.Path)
	for ext := range staticAssetExts {
		if strings.HasSuffix(path, ext) {
			return true
		}
	}
	return false
}

func (m *Matcher) Match(candidate string) bool {
	if m == nil {
		return false
	}

	parsed, err := url.Parse(candidate)
	if err != nil {
		return false
	}
	if parsed.Scheme != "http" && parsed.Scheme != "https" {
		return false
	}
	if parsed.Host == "" {
		return false
	}

	if isStaticAsset(parsed) {
		return false
	}

	if !m.withinScope(parsed) {
		return false
	}

	target := normalizePath(parsed.Path)
	if m.matchFullURL {
		target = candidate
	}

	if len(m.include) > 0 {
		allowed := false
		for _, pattern := range m.include {
			if pattern.Match(target) {
				allowed = true
				break
			}
		}
		if !allowed {
			return false
		}
	}

	for _, pattern := range m.exclude {
		if pattern.Match(target) {
			return false
		}
	}

	return true
}

func (m *Matcher) withinScope(candidate *url.URL) bool {
	candidateHost := strings.ToLower(candidate.Hostname())
	startHost := strings.ToLower(m.startURL.Hostname())

	if m.spec.AllowExternalLinks {
		return true
	}

	if candidateHost == startHost {
		if m.spec.CrawlEntireDomain {
			return true
		}
		return withinPathScope(normalizePath(candidate.Path), m.startPath)
	}

	if m.spec.AllowSubdomains && BaseDomain(candidateHost) == m.startBaseDomain {
		return true
	}

	return false
}

func withinPathScope(candidatePath, startPath string) bool {
	if startPath == "/" {
		return true
	}
	if candidatePath == startPath {
		return true
	}
	return strings.HasPrefix(candidatePath, startPath+"/")
}

func compilePatterns(patterns []string, regexMode bool) ([]compiledPattern, error) {
	if len(patterns) == 0 {
		return nil, nil
	}

	compiled := make([]compiledPattern, 0, len(patterns))
	for _, raw := range patterns {
		pattern := strings.TrimSpace(raw)
		if pattern == "" {
			continue
		}
		if regexMode {
			re, err := CompileRegexp(pattern)
			if err != nil {
				return nil, fmt.Errorf("invalid regex pattern %q: %w", pattern, err)
			}
			compiled = append(compiled, regexPattern{re: re})
			continue
		}

		globPatternValue, err := glob.Compile(pattern, '/')
		if err != nil {
			return nil, fmt.Errorf("invalid path pattern %q: %w", pattern, err)
		}
		compiled = append(compiled, globPattern{glob: globPatternValue, raw: pattern})
	}

	return compiled, nil
}
