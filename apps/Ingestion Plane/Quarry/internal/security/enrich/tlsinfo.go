package enrich

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"fmt"
	"net"
	"net/url"
	"time"
)

// TLSInfo contains detailed TLS/SSL certificate information
type TLSInfo struct {
	Host             string             `json:"host"`
	Port             string             `json:"port"`
	TLSVersion       string             `json:"tls_version"`
	CipherSuite      string             `json:"cipher_suite"`
	Certificate      *CertificateInfo   `json:"certificate"`
	CertificateChain []*CertificateInfo `json:"certificate_chain"`
	SecurityIssues   []SecurityIssue    `json:"security_issues"`
	CheckedAt        time.Time          `json:"checked_at"`
	ResponseTime     time.Duration      `json:"response_time"`
	IsValid          bool               `json:"is_valid"`
	TrustScore       float64            `json:"trust_score"`
}

// CertificateInfo contains certificate details
type CertificateInfo struct {
	Subject            string            `json:"subject"`
	Issuer             string            `json:"issuer"`
	SerialNumber       string            `json:"serial_number"`
	NotBefore          time.Time         `json:"not_before"`
	NotAfter           time.Time         `json:"not_after"`
	DNSNames           []string          `json:"dns_names"`
	IPAddresses        []string          `json:"ip_addresses"`
	SignatureAlgorithm string            `json:"signature_algorithm"`
	PublicKeyAlgorithm string            `json:"public_key_algorithm"`
	KeySize            int               `json:"key_size"`
	IsCA               bool              `json:"is_ca"`
	IsSelfSigned       bool              `json:"is_self_signed"`
	IsExpired          bool              `json:"is_expired"`
	DaysUntilExpiry    int               `json:"days_until_expiry"`
	Fingerprint        string            `json:"fingerprint"`
	Extensions         map[string]string `json:"extensions"`
}

// SecurityIssue represents a TLS security concern
type SecurityIssue struct {
	Type        string  `json:"type"`
	Severity    string  `json:"severity"`
	Description string  `json:"description"`
	Impact      float64 `json:"impact"`
}

// TLSAnalyzer handles TLS certificate analysis
type TLSAnalyzer struct {
	Timeout time.Duration
}

// NewTLSAnalyzer creates a new TLS analyzer
func NewTLSAnalyzer() *TLSAnalyzer {
	return &TLSAnalyzer{
		Timeout: 10 * time.Second,
	}
}

// AnalyzeTLS performs comprehensive TLS analysis
func (ta *TLSAnalyzer) AnalyzeTLS(ctx context.Context, targetURL string) (*TLSInfo, error) {
	parsedURL, err := url.Parse(targetURL)
	if err != nil {
		return nil, fmt.Errorf("invalid URL: %w", err)
	}

	host := parsedURL.Hostname()
	port := parsedURL.Port()
	if port == "" {
		if parsedURL.Scheme == "https" {
			port = "443"
		} else {
			port = "80"
		}
	}

	startTime := time.Now()

	// Connect with TLS
	dialer := &net.Dialer{
		Timeout: ta.Timeout,
	}

	conn, err := tls.DialWithDialer(dialer, "tcp", fmt.Sprintf("%s:%s", host, port), &tls.Config{
		InsecureSkipVerify: false,
	})
	if err != nil {
		return nil, fmt.Errorf("TLS connection failed: %w", err)
	}
	defer conn.Close()

	state := conn.ConnectionState()
	responseTime := time.Since(startTime)

	tlsInfo := &TLSInfo{
		Host:             host,
		Port:             port,
		TLSVersion:       tlsVersionString(state.Version),
		CipherSuite:      tls.CipherSuiteName(state.CipherSuite),
		CertificateChain: make([]*CertificateInfo, 0),
		SecurityIssues:   make([]SecurityIssue, 0),
		CheckedAt:        startTime,
		ResponseTime:     responseTime,
		IsValid:          true,
	}

	// Analyze certificates
	for _, cert := range state.PeerCertificates {
		certInfo := ta.analyzeCertificate(cert)
		tlsInfo.CertificateChain = append(tlsInfo.CertificateChain, certInfo)
	}

	if len(tlsInfo.CertificateChain) > 0 {
		tlsInfo.Certificate = tlsInfo.CertificateChain[0]
	}

	return tlsInfo, nil
}

func (ta *TLSAnalyzer) analyzeCertificate(cert *x509.Certificate) *CertificateInfo {
	now := time.Now()
	daysUntilExpiry := int(cert.NotAfter.Sub(now).Hours() / 24)

	certInfo := &CertificateInfo{
		Subject:            cert.Subject.String(),
		Issuer:             cert.Issuer.String(),
		SerialNumber:       cert.SerialNumber.String(),
		NotBefore:          cert.NotBefore,
		NotAfter:           cert.NotAfter,
		DNSNames:           cert.DNSNames,
		SignatureAlgorithm: cert.SignatureAlgorithm.String(),
		PublicKeyAlgorithm: cert.PublicKeyAlgorithm.String(),
		IsCA:               cert.IsCA,
		IsExpired:          now.After(cert.NotAfter),
		DaysUntilExpiry:    daysUntilExpiry,
		Extensions:         make(map[string]string),
	}

	// Convert IP addresses
	for _, ip := range cert.IPAddresses {
		certInfo.IPAddresses = append(certInfo.IPAddresses, ip.String())
	}

	return certInfo
}

func tlsVersionString(version uint16) string {
	switch version {
	case tls.VersionTLS10:
		return "TLS 1.0"
	case tls.VersionTLS11:
		return "TLS 1.1"
	case tls.VersionTLS12:
		return "TLS 1.2"
	case tls.VersionTLS13:
		return "TLS 1.3"
	default:
		return fmt.Sprintf("Unknown (%d)", version)
	}
}
