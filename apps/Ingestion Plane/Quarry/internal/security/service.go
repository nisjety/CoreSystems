package security

import (
	"context"
	"net/url"
	"time"

	"github.com/patrickmn/go-cache"
	"github.com/triodelab/quarry/internal/config"
	"github.com/triodelab/quarry/internal/security/enrich"
	"github.com/triodelab/quarry/internal/security/heur"
	"github.com/triodelab/quarry/internal/security/repoprovider"
)

type Assessment struct {
	URL             string                          `json:"url"`
	Domain          string                          `json:"domain"`
	SuspicionScore  float64                         `json:"suspicionScore"`
	ProviderResults []repoprovider.ReputationResult `json:"providerResults"`
	DNS             *enrich.DNSInfo                 `json:"dns,omitempty"`
	TLS             *enrich.TLSInfo                 `json:"tls,omitempty"`
	Blocked         bool                            `json:"blocked"`
	BlockReason     string                          `json:"blockReason,omitempty"`
	CheckedAt       time.Time                       `json:"checkedAt"`
}

type Service struct {
	urlAnalyzer       *heur.URLAnalyzer
	dnsAnalyzer       *enrich.DNSAnalyzer
	tlsAnalyzer       *enrich.TLSAnalyzer
	urlHaus           *repoprovider.URLHausProvider
	safeBrowsing      *repoprovider.SafeBrowsingProvider
	securityThreshold float64

	// Cache for reputation lookups
	repCache *cache.Cache
}

func NewService(cfg *config.Config) *Service {
	threshold := 0.8
	if cfg != nil && cfg.SecurityBlockThreshold > 0 {
		threshold = cfg.SecurityBlockThreshold
	}

	// Create cache with default expiration of 30 minutes, cleanup every 1 hr
	c := cache.New(30*time.Minute, 1*time.Hour)

	var safeBrowsing *repoprovider.SafeBrowsingProvider
	if cfg != nil {
		safeBrowsing = repoprovider.NewSafeBrowsingProvider(cfg.GoogleSafeBrowsingAPIKey)
	} else {
		safeBrowsing = repoprovider.NewSafeBrowsingProvider("")
	}

	return &Service{
		urlAnalyzer:       heur.NewURLAnalyzer(),
		dnsAnalyzer:       enrich.NewDNSAnalyzer(),
		tlsAnalyzer:       enrich.NewTLSAnalyzer(),
		urlHaus:           repoprovider.NewURLHausProvider(),
		safeBrowsing:      safeBrowsing,
		securityThreshold: threshold,
		repCache:          c,
	}
}

func (s *Service) AssessURL(ctx context.Context, targetURL string) (*Assessment, error) {
	// Check cache
	if obj, found := s.repCache.Get(targetURL); found {
		if assessment, ok := obj.(*Assessment); ok {
			return assessment, nil
		}
	}

	parsed, err := url.Parse(targetURL)
	if err != nil {
		return nil, err
	}

	assessment := &Assessment{
		URL:             targetURL,
		Domain:          parsed.Hostname(),
		ProviderResults: make([]repoprovider.ReputationResult, 0, 2),
		CheckedAt:       time.Now(),
	}

	if s.urlAnalyzer != nil {
		sig, err := s.urlAnalyzer.AnalyzeURL(targetURL)
		if err == nil && sig != nil {
			assessment.SuspicionScore = sig.SuspicionScore
		}
	}

	if s.dnsAnalyzer != nil {
		if dns, err := s.dnsAnalyzer.AnalyzeDNS(ctx, targetURL); err == nil {
			assessment.DNS = dns
		}
	}

	if parsed.Scheme == "https" && s.tlsAnalyzer != nil {
		if tlsInfo, err := s.tlsAnalyzer.AnalyzeTLS(ctx, targetURL); err == nil {
			assessment.TLS = tlsInfo
		}
	}

	if s.urlHaus != nil {
		if result, err := s.urlHaus.CheckURL(ctx, targetURL); err == nil && result != nil {
			assessment.ProviderResults = append(assessment.ProviderResults, *result)
		}
	}

	if s.safeBrowsing != nil && s.safeBrowsing.IsAvailable() {
		if result, err := s.safeBrowsing.CheckURL(ctx, targetURL); err == nil && result != nil {
			assessment.ProviderResults = append(assessment.ProviderResults, *result)
		}
	}

	maxProviderScore := 0.0
	for _, p := range assessment.ProviderResults {
		if p.Score > maxProviderScore {
			maxProviderScore = p.Score
		}
	}

	overall := assessment.SuspicionScore
	if maxProviderScore > overall {
		overall = maxProviderScore
	}

	if overall >= s.securityThreshold {
		assessment.Blocked = true
		assessment.BlockReason = "security threshold exceeded"
	}

	// Cache the result
	s.repCache.Set(targetURL, assessment, cache.DefaultExpiration)

	return assessment, nil
}
