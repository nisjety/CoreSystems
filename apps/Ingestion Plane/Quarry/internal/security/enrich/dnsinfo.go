package enrich

import (
	"context"
	"fmt"
	"net"
	"net/url"
	"strings"
	"time"
)

// DNSInfo contains DNS lookup and analysis results
type DNSInfo struct {
	Domain       string        `json:"domain"`
	IPAddresses  []string      `json:"ip_addresses"`
	IPv4         []string      `json:"ipv4_addresses"`
	IPv6         []string      `json:"ipv6_addresses"`
	CNAMEs       []string      `json:"cnames"`
	MXRecords    []MXRecord    `json:"mx_records"`
	TXTRecords   []string      `json:"txt_records"`
	NSRecords    []string      `json:"ns_records"`
	ResponseTime time.Duration `json:"response_time"`
	CheckedAt    time.Time     `json:"checked_at"`
	IsSuspicious bool          `json:"is_suspicious"`
	Flags        []DNSFlag     `json:"flags"`
}

// MXRecord represents a mail exchange record
type MXRecord struct {
	Host     string `json:"host"`
	Priority int    `json:"priority"`
}

// DNSFlag represents suspicious DNS characteristics
type DNSFlag struct {
	Type        string  `json:"type"`
	Severity    string  `json:"severity"`
	Description string  `json:"description"`
	Impact      float64 `json:"impact"`
}

// DNSAnalyzer handles DNS analysis
type DNSAnalyzer struct {
	Timeout time.Duration
}

// NewDNSAnalyzer creates a new DNS analyzer
func NewDNSAnalyzer() *DNSAnalyzer {
	return &DNSAnalyzer{
		Timeout: 5 * time.Second,
	}
}

// AnalyzeDNS performs comprehensive DNS analysis
func (da *DNSAnalyzer) AnalyzeDNS(ctx context.Context, targetURL string) (*DNSInfo, error) {
	startTime := time.Now()

	parsedURL, err := url.Parse(targetURL)
	if err != nil {
		return nil, fmt.Errorf("invalid URL: %w", err)
	}

	domain := parsedURL.Hostname()
	if domain == "" {
		return nil, fmt.Errorf("no hostname in URL")
	}

	dnsInfo := &DNSInfo{
		Domain:    domain,
		CheckedAt: startTime,
		Flags:     make([]DNSFlag, 0),
	}

	// Create context with timeout
	ctx, cancel := context.WithTimeout(ctx, da.Timeout)
	defer cancel()

	// Resolve IP addresses
	ips, err := net.DefaultResolver.LookupIPAddr(ctx, domain)
	if err != nil {
		dnsInfo.Flags = append(dnsInfo.Flags, DNSFlag{
			Type:        "resolution_failed",
			Severity:    "Critical",
			Description: fmt.Sprintf("DNS resolution failed: %v", err),
			Impact:      1.0,
		})
	} else {
		for _, ip := range ips {
			ipStr := ip.IP.String()
			dnsInfo.IPAddresses = append(dnsInfo.IPAddresses, ipStr)

			if ip.IP.To4() != nil {
				dnsInfo.IPv4 = append(dnsInfo.IPv4, ipStr)
			} else {
				dnsInfo.IPv6 = append(dnsInfo.IPv6, ipStr)
			}
		}
	}

	// Lookup CNAME records
	cname, err := net.DefaultResolver.LookupCNAME(ctx, domain)
	if err == nil && cname != domain && cname != "" {
		dnsInfo.CNAMEs = append(dnsInfo.CNAMEs, strings.TrimSuffix(cname, "."))
	}

	// Lookup MX records
	mxRecords, err := net.DefaultResolver.LookupMX(ctx, domain)
	if err == nil {
		for _, mx := range mxRecords {
			dnsInfo.MXRecords = append(dnsInfo.MXRecords, MXRecord{
				Host:     strings.TrimSuffix(mx.Host, "."),
				Priority: int(mx.Pref),
			})
		}
	}

	// Lookup TXT records
	txtRecords, err := net.DefaultResolver.LookupTXT(ctx, domain)
	if err == nil {
		dnsInfo.TXTRecords = txtRecords
	}

	// Lookup NS records
	nsRecords, err := net.DefaultResolver.LookupNS(ctx, domain)
	if err == nil {
		for _, ns := range nsRecords {
			dnsInfo.NSRecords = append(dnsInfo.NSRecords, strings.TrimSuffix(ns.Host, "."))
		}
	}

	dnsInfo.ResponseTime = time.Since(startTime)

	// Perform suspicious analysis
	da.analyzeSuspiciousPatterns(dnsInfo)

	// Set overall suspicious flag
	dnsInfo.IsSuspicious = da.calculateSuspiciousScore(dnsInfo) > 0.5

	return dnsInfo, nil
}

// analyzeSuspiciousPatterns identifies suspicious DNS characteristics
func (da *DNSAnalyzer) analyzeSuspiciousPatterns(dnsInfo *DNSInfo) {
	domain := dnsInfo.Domain

	// Check for suspicious TLDs
	suspiciousTLDs := []string{
		".tk", ".ml", ".ga", ".cf", ".pw", ".top", ".click", ".download",
		".science", ".work", ".party", ".racing", ".win", ".bid",
	}

	for _, tld := range suspiciousTLDs {
		if strings.HasSuffix(domain, tld) {
			dnsInfo.Flags = append(dnsInfo.Flags, DNSFlag{
				Type:        "suspicious_tld",
				Severity:    "Medium",
				Description: fmt.Sprintf("Uses suspicious TLD: %s", tld),
				Impact:      0.6,
			})
			break
		}
	}

	// Check for punycode/IDN homograph attacks
	if strings.Contains(domain, "xn--") {
		dnsInfo.Flags = append(dnsInfo.Flags, DNSFlag{
			Type:        "punycode_domain",
			Severity:    "Medium",
			Description: "Domain contains punycode (possible homograph attack)",
			Impact:      0.5,
		})
	}

	// Check for suspicious patterns in domain name
	suspiciousPatterns := []string{
		"secure", "verify", "account", "update", "login", "bank",
		"paypal", "amazon", "apple", "microsoft", "google",
	}

	lowerDomain := strings.ToLower(domain)
	for _, pattern := range suspiciousPatterns {
		if strings.Contains(lowerDomain, pattern) && !strings.HasSuffix(lowerDomain, pattern+".com") {
			dnsInfo.Flags = append(dnsInfo.Flags, DNSFlag{
				Type:        "suspicious_keywords",
				Severity:    "Medium",
				Description: fmt.Sprintf("Domain contains suspicious keyword: %s", pattern),
				Impact:      0.4,
			})
		}
	}

	// Check for excessive subdomain levels
	levels := strings.Count(domain, ".")
	if levels > 4 {
		dnsInfo.Flags = append(dnsInfo.Flags, DNSFlag{
			Type:        "excessive_subdomains",
			Severity:    "Low",
			Description: fmt.Sprintf("Domain has %d levels (suspicious)", levels+1),
			Impact:      0.3,
		})
	}

	// Check for very long domain names
	if len(domain) > 50 {
		dnsInfo.Flags = append(dnsInfo.Flags, DNSFlag{
			Type:        "long_domain",
			Severity:    "Low",
			Description: fmt.Sprintf("Very long domain name (%d characters)", len(domain)),
			Impact:      0.2,
		})
	}

	// Check for domains with many numbers
	numCount := 0
	for _, char := range domain {
		if char >= '0' && char <= '9' {
			numCount++
		}
	}

	if float64(numCount)/float64(len(domain)) > 0.3 {
		dnsInfo.Flags = append(dnsInfo.Flags, DNSFlag{
			Type:        "numeric_heavy",
			Severity:    "Low",
			Description: "Domain contains many numbers (suspicious pattern)",
			Impact:      0.3,
		})
	}

	// Check for missing common records (suspicious for legitimate sites)
	if len(dnsInfo.MXRecords) == 0 && len(dnsInfo.TXTRecords) == 0 {
		dnsInfo.Flags = append(dnsInfo.Flags, DNSFlag{
			Type:        "minimal_dns_setup",
			Severity:    "Low",
			Description: "Missing common DNS records (MX, TXT)",
			Impact:      0.2,
		})
	}

	// Check for fast flux indicators (multiple A records)
	if len(dnsInfo.IPv4) > 10 {
		dnsInfo.Flags = append(dnsInfo.Flags, DNSFlag{
			Type:        "fast_flux_pattern",
			Severity:    "High",
			Description: fmt.Sprintf("Many IP addresses (%d) - possible fast flux", len(dnsInfo.IPv4)),
			Impact:      0.8,
		})
	}

	// Check for private/internal IP addresses
	for _, ip := range dnsInfo.IPv4 {
		if da.isPrivateIP(ip) {
			dnsInfo.Flags = append(dnsInfo.Flags, DNSFlag{
				Type:        "private_ip",
				Severity:    "Medium",
				Description: fmt.Sprintf("Resolves to private IP: %s", ip),
				Impact:      0.5,
			})
		}
	}

	// Check for localhost resolution
	for _, ip := range dnsInfo.IPv4 {
		if ip == "127.0.0.1" || strings.HasPrefix(ip, "127.") {
			dnsInfo.Flags = append(dnsInfo.Flags, DNSFlag{
				Type:        "localhost_resolution",
				Severity:    "High",
				Description: "Domain resolves to localhost",
				Impact:      0.9,
			})
		}
	}
}

// calculateSuspiciousScore calculates overall suspiciousness (0.0 to 1.0)
func (da *DNSAnalyzer) calculateSuspiciousScore(dnsInfo *DNSInfo) float64 {
	if len(dnsInfo.Flags) == 0 {
		return 0.0
	}

	totalImpact := 0.0
	for _, flag := range dnsInfo.Flags {
		totalImpact += flag.Impact
	}

	// Normalize by number of flags (average impact)
	score := totalImpact / float64(len(dnsInfo.Flags))

	// Cap at 1.0
	if score > 1.0 {
		score = 1.0
	}

	return score
}

// isPrivateIP checks if an IP address is in private ranges
func (da *DNSAnalyzer) isPrivateIP(ipStr string) bool {
	ip := net.ParseIP(ipStr)
	if ip == nil {
		return false
	}

	// Check IPv4 private ranges
	if ip.To4() != nil {
		// 10.0.0.0/8
		if ip[12] == 10 {
			return true
		}
		// 172.16.0.0/12
		if ip[12] == 172 && ip[13] >= 16 && ip[13] <= 31 {
			return true
		}
		// 192.168.0.0/16
		if ip[12] == 192 && ip[13] == 168 {
			return true
		}
	}

	return false
}

// GetGeoLocation attempts to determine approximate geolocation of IP addresses
func (da *DNSAnalyzer) GetGeoLocation(ctx context.Context, dnsInfo *DNSInfo) {
	// This would typically integrate with a GeoIP service
	// For now, we'll just add basic regional identification

	for _, ip := range dnsInfo.IPv4 {
		// Add basic geo info as metadata (placeholder)
		if strings.HasPrefix(ip, "8.8.") || strings.HasPrefix(ip, "1.1.") {
			// Public DNS servers
			continue
		}

		// In a real implementation, you would call a GeoIP API here
		// and add location information to the DNS info
	}
}
