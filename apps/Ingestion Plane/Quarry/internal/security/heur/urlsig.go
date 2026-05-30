package heur

import (
	"net/url"
	"regexp"
	"strings"
)

// URLSignature represents heuristic analysis of a URL
type URLSignature struct {
	URL             string              `json:"url"`
	Domain          string              `json:"domain"`
	Path            string              `json:"path"`
	Query           string              `json:"query"`
	Fragment        string              `json:"fragment"`
	Scheme          string              `json:"scheme"`
	Port            string              `json:"port"`
	TLD             string              `json:"tld"`
	SubdomainCount  int                 `json:"subdomain_count"`
	PathSegments    []string            `json:"path_segments"`
	QueryParams     map[string]string   `json:"query_params"`
	Characteristics []URLCharacteristic `json:"characteristics"`
	SuspicionScore  float64             `json:"suspicion_score"`
	RiskFactors     []RiskFactor        `json:"risk_factors"`
}

// URLCharacteristic represents a specific URL trait
type URLCharacteristic struct {
	Type        string  `json:"type"`
	Value       string  `json:"value"`
	Score       float64 `json:"score"` // 0.0 (safe) to 1.0 (suspicious)
	Description string  `json:"description"`
}

// RiskFactor represents a specific security risk
type RiskFactor struct {
	Category    string  `json:"category"`
	Severity    string  `json:"severity"` // Low, Medium, High, Critical
	Description string  `json:"description"`
	Impact      float64 `json:"impact"`     // 0.0 to 1.0
	Confidence  float64 `json:"confidence"` // 0.0 to 1.0
}

// URLAnalyzer performs heuristic URL analysis
type URLAnalyzer struct {
	// Compiled regexes for performance
	ipRegex         *regexp.Regexp
	suspiciousRegex *regexp.Regexp
	homographRegex  *regexp.Regexp
}

// NewURLAnalyzer creates a new URL analyzer with compiled patterns
func NewURLAnalyzer() *URLAnalyzer {
	return &URLAnalyzer{
		ipRegex:         regexp.MustCompile(`^https?://(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})`),
		suspiciousRegex: regexp.MustCompile(`(bit\.ly|tinyurl|goo\.gl|t\.co|short|redirect|r\.php|go\.php)`),
		homographRegex:  regexp.MustCompile(`[а-я]|[ა-ჰ]|[α-ω]`), // Cyrillic, Georgian, Greek
	}
}

// AnalyzeURL performs comprehensive heuristic analysis
func (ua *URLAnalyzer) AnalyzeURL(targetURL string) (*URLSignature, error) {
	parsedURL, err := url.Parse(targetURL)
	if err != nil {
		return nil, err
	}

	signature := &URLSignature{
		URL:             targetURL,
		Domain:          parsedURL.Hostname(),
		Path:            parsedURL.Path,
		Query:           parsedURL.RawQuery,
		Fragment:        parsedURL.Fragment,
		Scheme:          parsedURL.Scheme,
		Port:            parsedURL.Port(),
		QueryParams:     make(map[string]string),
		Characteristics: make([]URLCharacteristic, 0),
		RiskFactors:     make([]RiskFactor, 0),
	}

	// Parse query parameters
	queryValues := parsedURL.Query()
	for key, values := range queryValues {
		if len(values) > 0 {
			signature.QueryParams[key] = values[0]
		}
	}

	// Extract TLD and path segments
	ua.extractStructuralInfo(signature)

	// Perform various analyses
	ua.analyzeScheme(signature)
	ua.analyzeDomain(signature)
	ua.analyzePath(signature)
	ua.analyzeQuery(signature)
	ua.analyzeLength(signature)
	ua.analyzeCharacters(signature)
	ua.analyzePatterns(signature)
	ua.analyzeSuspiciousKeywords(signature)

	// Calculate final scores
	signature.SuspicionScore = ua.calculateSuspicionScore(signature)

	return signature, nil
}

// extractStructuralInfo extracts basic structural information
func (ua *URLAnalyzer) extractStructuralInfo(sig *URLSignature) {
	// Extract TLD
	parts := strings.Split(sig.Domain, ".")
	if len(parts) > 0 {
		sig.TLD = parts[len(parts)-1]
		sig.SubdomainCount = len(parts) - 2 // domain.tld = 0 subdomains
		if sig.SubdomainCount < 0 {
			sig.SubdomainCount = 0
		}
	}

	// Extract path segments
	if sig.Path != "" && sig.Path != "/" {
		segments := strings.Split(strings.Trim(sig.Path, "/"), "/")
		for _, segment := range segments {
			if segment != "" {
				sig.PathSegments = append(sig.PathSegments, segment)
			}
		}
	}
}

// analyzeScheme checks URL scheme
func (ua *URLAnalyzer) analyzeScheme(sig *URLSignature) {
	switch sig.Scheme {
	case "http":
		sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
			Category:    "encryption",
			Severity:    "Medium",
			Description: "Uses unencrypted HTTP protocol",
			Impact:      0.6,
			Confidence:  1.0,
		})

	case "https":
		sig.Characteristics = append(sig.Characteristics, URLCharacteristic{
			Type:        "scheme",
			Value:       "https",
			Score:       0.0,
			Description: "Uses secure HTTPS protocol",
		})

	case "ftp", "file", "data":
		sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
			Category:    "scheme",
			Severity:    "Medium",
			Description: "Uses unusual protocol: " + sig.Scheme,
			Impact:      0.5,
			Confidence:  0.8,
		})
	}
}

// analyzeDomain performs domain-specific analysis
func (ua *URLAnalyzer) analyzeDomain(sig *URLSignature) {
	domain := strings.ToLower(sig.Domain)

	// Check for IP address instead of domain
	if ua.ipRegex.MatchString("http://"+sig.Domain) || ua.ipRegex.MatchString("https://"+sig.Domain) {
		sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
			Category:    "domain",
			Severity:    "High",
			Description: "Uses IP address instead of domain name",
			Impact:      0.8,
			Confidence:  1.0,
		})
	}

	// Check for suspicious TLDs
	suspiciousTLDs := map[string]float64{
		"tk": 0.9, "ml": 0.9, "ga": 0.9, "cf": 0.8,
		"pw": 0.7, "top": 0.6, "click": 0.8, "download": 0.9,
		"science": 0.7, "work": 0.5, "party": 0.6, "racing": 0.7,
		"win": 0.6, "bid": 0.7, "loan": 0.8, "cricket": 0.6,
	}

	if score, isSuspicious := suspiciousTLDs[sig.TLD]; isSuspicious {
		sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
			Category:    "domain",
			Severity:    ua.getSeverity(score),
			Description: "Uses suspicious TLD: ." + sig.TLD,
			Impact:      score,
			Confidence:  0.8,
		})
	}

	// Check for homograph attacks
	if ua.homographRegex.MatchString(domain) {
		sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
			Category:    "domain",
			Severity:    "High",
			Description: "Contains non-Latin characters (possible homograph attack)",
			Impact:      0.8,
			Confidence:  0.7,
		})
	}

	// Check subdomain count
	if sig.SubdomainCount > 3 {
		sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
			Category:    "domain",
			Severity:    "Medium",
			Description: "Excessive subdomains (level " + string(rune(sig.SubdomainCount+2)) + ")",
			Impact:      0.4 + float64(sig.SubdomainCount-3)*0.1,
			Confidence:  0.6,
		})
	}

	// Check for domain squatting patterns
	ua.checkDomainSquatting(sig, domain)
}

// checkDomainSquatting identifies potential typosquatting
func (ua *URLAnalyzer) checkDomainSquatting(sig *URLSignature, domain string) {
	// Popular domains to check against
	popularDomains := []string{
		"google", "facebook", "amazon", "microsoft", "apple", "twitter",
		"linkedin", "instagram", "youtube", "netflix", "paypal", "ebay",
		"yahoo", "reddit", "wikipedia", "github", "stackoverflow",
	}

	for _, popular := range popularDomains {
		// Check for character substitution
		if ua.isLikelyTyposquat(domain, popular) {
			sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
				Category:    "domain",
				Severity:    "High",
				Description: "Possible typosquatting of " + popular,
				Impact:      0.9,
				Confidence:  0.8,
			})
			break
		}
	}
}

// isLikelyTyposquat checks if domain is similar to a popular domain
func (ua *URLAnalyzer) isLikelyTyposquat(domain, popular string) bool {
	// Extract main domain without subdomains and TLD for comparison
	domainParts := strings.Split(domain, ".")
	if len(domainParts) < 2 {
		return false
	}

	// Get the main domain part (second-level domain)
	mainDomain := domainParts[len(domainParts)-2]

	// Don't flag exact matches or legitimate domains
	if mainDomain == popular {
		return false
	}

	// Don't flag if the domain is much longer (likely legitimate)
	if len(mainDomain) > len(popular)+3 {
		return false
	}

	// Check for character substitution only if lengths are similar
	if abs(len(mainDomain)-len(popular)) > 2 {
		return false
	}

	// Character substitution check (basic)
	substitutions := map[rune][]rune{
		'o': {'0'},
		'i': {'1', 'l'},
		'e': {'3'},
		'a': {'@'},
		's': {'$'},
	}

	for original, replacements := range substitutions {
		for _, replacement := range replacements {
			modified := strings.ReplaceAll(popular, string(original), string(replacement))
			if mainDomain == modified {
				return true
			}
		}
	}

	// Simple character insertion/deletion check
	if ua.isOneEditDistance(mainDomain, popular) && len(mainDomain) > 4 {
		return true
	}

	return false
}

// Helper function to calculate absolute difference
func abs(a int) int {
	if a < 0 {
		return -a
	}
	return a
}

// isOneEditDistance checks if strings differ by exactly one edit
func (ua *URLAnalyzer) isOneEditDistance(s1, s2 string) bool {
	if abs(len(s1)-len(s2)) > 1 {
		return false
	}

	if len(s1) > len(s2) {
		s1, s2 = s2, s1 // Ensure s1 is shorter or equal
	}

	for i := 0; i < len(s1); i++ {
		if s1[i] != s2[i] {
			if len(s1) == len(s2) {
				return s1[i+1:] == s2[i+1:] // Replace
			} else {
				return s1[i:] == s2[i+1:] // Insert
			}
		}
	}

	return len(s1)+1 == len(s2) // Insert at end
}

// analyzePath examines URL path for suspicious patterns
func (ua *URLAnalyzer) analyzePath(sig *URLSignature) {
	path := strings.ToLower(sig.Path)

	// Suspicious path patterns
	suspiciousPatterns := map[string]float64{
		"admin":    0.3,
		"login":    0.2,
		"secure":   0.4,
		"verify":   0.6,
		"update":   0.5,
		"confirm":  0.5,
		"account":  0.4,
		"billing":  0.4,
		"payment":  0.5,
		"download": 0.3,
		"redirect": 0.7,
		"r.php":    0.8,
		"go.php":   0.8,
		"link.php": 0.7,
	}

	for pattern, score := range suspiciousPatterns {
		if strings.Contains(path, pattern) {
			sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
				Category:    "path",
				Severity:    ua.getSeverity(score),
				Description: "Suspicious path component: " + pattern,
				Impact:      score,
				Confidence:  0.6,
			})
		}
	}

	// Check for excessive path depth
	if len(sig.PathSegments) > 5 {
		sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
			Category:    "path",
			Severity:    "Low",
			Description: "Deeply nested path structure",
			Impact:      0.3,
			Confidence:  0.4,
		})
	}
}

// analyzeQuery examines query parameters
func (ua *URLAnalyzer) analyzeQuery(sig *URLSignature) {
	if sig.Query == "" {
		return
	}

	// Suspicious query parameters
	suspiciousParams := map[string]float64{
		"redirect": 0.8,
		"url":      0.6,
		"link":     0.6,
		"goto":     0.7,
		"target":   0.5,
		"ref":      0.3,
		"return":   0.4,
	}

	for param := range sig.QueryParams {
		paramLower := strings.ToLower(param)
		if score, isSuspicious := suspiciousParams[paramLower]; isSuspicious {
			sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
				Category:    "query",
				Severity:    ua.getSeverity(score),
				Description: "Suspicious query parameter: " + param,
				Impact:      score,
				Confidence:  0.7,
			})
		}
	}

	// Check for encoded URLs in parameters
	for _, value := range sig.QueryParams {
		if strings.Contains(value, "http") || strings.Contains(value, "%3A%2F%2F") {
			sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
				Category:    "query",
				Severity:    "Medium",
				Description: "Query parameter contains URL (possible redirect)",
				Impact:      0.6,
				Confidence:  0.8,
			})
		}
	}
}

// analyzeLength checks URL length characteristics
func (ua *URLAnalyzer) analyzeLength(sig *URLSignature) {
	urlLength := len(sig.URL)

	sig.Characteristics = append(sig.Characteristics, URLCharacteristic{
		Type:        "length",
		Value:       string(rune(urlLength)),
		Score:       ua.getLengthScore(urlLength),
		Description: "URL length analysis",
	})

	if urlLength > 2048 {
		sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
			Category:    "structure",
			Severity:    "High",
			Description: "Extremely long URL (possible attack)",
			Impact:      0.8,
			Confidence:  0.9,
		})
	} else if urlLength > 1000 {
		sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
			Category:    "structure",
			Severity:    "Medium",
			Description: "Very long URL",
			Impact:      0.4,
			Confidence:  0.6,
		})
	}
}

// analyzeCharacters examines character composition
func (ua *URLAnalyzer) analyzeCharacters(sig *URLSignature) {
	url := sig.URL

	// Count character types
	var digits, letters, special, unicodeChars int
	for _, char := range url {
		switch {
		case char >= '0' && char <= '9':
			digits++
		case (char >= 'a' && char <= 'z') || (char >= 'A' && char <= 'Z'):
			letters++
		case char > 127:
			unicodeChars++
		default:
			special++
		}
	}

	total := len(url)
	if total == 0 {
		return
	}

	// Check digit ratio
	digitRatio := float64(digits) / float64(total)
	if digitRatio > 0.3 {
		sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
			Category:    "characters",
			Severity:    "Low",
			Description: "High ratio of digits in URL",
			Impact:      0.3,
			Confidence:  0.5,
		})
	}

	// Check unicode characters
	if unicodeChars > 0 {
		sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
			Category:    "characters",
			Severity:    "Medium",
			Description: "Contains Unicode characters",
			Impact:      0.5,
			Confidence:  0.7,
		})
	}
}

// analyzePatterns looks for suspicious patterns
func (ua *URLAnalyzer) analyzePatterns(sig *URLSignature) {
	url := strings.ToLower(sig.URL)

	// Check for URL shorteners
	if ua.suspiciousRegex.MatchString(url) {
		sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
			Category:    "pattern",
			Severity:    "Medium",
			Description: "Contains URL shortener or redirect service",
			Impact:      0.6,
			Confidence:  0.8,
		})
	}

	// Check for multiple subdomains with numbers
	domainParts := strings.Split(sig.Domain, ".")
	numberSubdomains := 0
	for i, part := range domainParts[:len(domainParts)-2] { // Exclude domain and TLD
		if regexp.MustCompile(`\d`).MatchString(part) {
			numberSubdomains++
		}
		_ = i
	}

	if numberSubdomains > 1 {
		sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
			Category:    "pattern",
			Severity:    "Medium",
			Description: "Multiple numeric subdomains",
			Impact:      0.5,
			Confidence:  0.6,
		})
	}
}

// analyzeSuspiciousKeywords checks for phishing-related keywords
func (ua *URLAnalyzer) analyzeSuspiciousKeywords(sig *URLSignature) {
	fullURL := strings.ToLower(sig.URL)
	domain := strings.ToLower(sig.Domain)

	// Skip keyword analysis for trusted domains
	trustedDomains := []string{
		"google.com", "microsoft.com", "apple.com", "amazon.com", "github.com",
		"facebook.com", "twitter.com", "linkedin.com", "instagram.com", "youtube.com",
		"paypal.com", "ebay.com", "netflix.com", "adobe.com", "stackoverflow.com",
	}

	for _, trusted := range trustedDomains {
		if strings.Contains(domain, trusted) {
			return // Skip keyword analysis for trusted domains
		}
	}

	// High-confidence phishing keywords (more specific)
	phishingKeywords := map[string]float64{
		"account-suspended": 0.9,
		"verify-account":    0.8,
		"account-locked":    0.9,
		"urgent-action":     0.8,
		"security-alert":    0.8,
		"confirm-identity":  0.7,
		"update-payment":    0.8,
		"billing-suspended": 0.8,
		"phishing":          0.9,
		"malware":           0.9,
		"virus":             0.9,
		"freemoney":         0.8,
		"bitcoin-generator": 0.9,
	}

	for keyword, score := range phishingKeywords {
		if strings.Contains(fullURL, keyword) {
			sig.RiskFactors = append(sig.RiskFactors, RiskFactor{
				Category:    "content",
				Severity:    ua.getSeverity(score),
				Description: "Contains phishing keyword: " + keyword,
				Impact:      score,
				Confidence:  0.8,
			})
		}
	}
}

// calculateSuspicionScore computes overall suspicion score
func (ua *URLAnalyzer) calculateSuspicionScore(sig *URLSignature) float64 {
	if len(sig.RiskFactors) == 0 {
		return 0.0
	}

	totalWeightedScore := 0.0
	totalWeight := 0.0

	for _, risk := range sig.RiskFactors {
		weight := risk.Confidence
		weightedScore := risk.Impact * weight

		totalWeightedScore += weightedScore
		totalWeight += weight
	}

	if totalWeight == 0 {
		return 0.0
	}

	score := totalWeightedScore / totalWeight

	// Cap at 1.0
	if score > 1.0 {
		score = 1.0
	}

	return score
}

// Helper functions

func (ua *URLAnalyzer) getLengthScore(length int) float64 {
	if length < 30 {
		return 0.0
	} else if length < 100 {
		return 0.1
	} else if length < 500 {
		return 0.3
	} else if length < 1000 {
		return 0.5
	} else {
		return 0.8
	}
}

func (ua *URLAnalyzer) getSeverity(score float64) string {
	if score >= 0.8 {
		return "High"
	} else if score >= 0.5 {
		return "Medium"
	} else {
		return "Low"
	}
}
