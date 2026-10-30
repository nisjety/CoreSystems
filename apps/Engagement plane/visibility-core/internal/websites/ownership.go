package websites

import (
	"context"
	"net/url"
	"regexp"
)

var dnsValuePattern = regexp.MustCompile(`^studio-verification=[A-Za-z0-9_-]{43}$`)

func dnsName(site string) string {
	parsed, _ := url.Parse(site)
	return "_studio-verification." + parsed.Hostname()
}
func (s *Service) Challenge(id string) (ChallengeResult, error) {
	return ChallengeResult{}, ErrUnavailable
}
func (s *Service) VerifyDNS(ctx context.Context, id string) (Website, error) {
	return Website{}, ErrUnavailable
}
