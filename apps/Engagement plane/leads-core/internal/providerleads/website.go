package providerleads

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/json"
	"errors"
	"io"
	"net/mail"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"
)

var ErrWebsiteInvalid = errors.New("invalid website inquiry")
var ErrWebsiteConflict = errors.New("submission identity conflict")
var websiteID = regexp.MustCompile(`^web_[a-f0-9]{32}$`)
var submissionID = regexp.MustCompile(`^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$`)
var organizationID = regexp.MustCompile(`^[A-Za-z0-9_-]{1,128}$`)
var protectedToken = regexp.MustCompile(`^[A-Za-z0-9_-]{32,256}$`)

type WebsiteInstallation struct {
	OrganizationID  string `json:"organizationId"`
	WebsiteID       string `json:"websiteId"`
	WebsiteRevision int64  `json:"websiteRevision"`
	Site            string `json:"site"`
	Environment     string `json:"environment"`
	WriteTokenFile  string `json:"writeTokenFile"`
	ReadTokenFile   string `json:"readTokenFile"`
	writeToken      string
	readToken       string
}
type WebsiteInquiry struct {
	SubmissionID string `json:"submissionId"`
	Name         string `json:"name"`
	Email        string `json:"email"`
	Company      string `json:"company"`
	ProjectType  string `json:"projectType"`
	Budget       string `json:"budget"`
	Message      string `json:"message"`
	Source       string `json:"source"`
	Locale       string `json:"locale"`
}
type WebsiteReceipt struct {
	ReceiptID  string `json:"receiptId"`
	AcceptedAt string `json:"acceptedAt"`
}
type WebsiteOutcomes struct {
	WebsiteID       string  `json:"websiteId"`
	WebsiteRevision int64   `json:"websiteRevision"`
	Site            string  `json:"site"`
	Environment     string  `json:"environment"`
	Source          string  `json:"source"`
	Status          string  `json:"status"`
	StartDate       string  `json:"startDate"`
	EndDate         string  `json:"endDate"`
	TimeZone        string  `json:"timeZone"`
	Inquiries       int64   `json:"inquiries"`
	Leads           int64   `json:"leads"`
	Bookings        *int64  `json:"bookings"`
	LastReceivedAt  *string `json:"lastReceivedAt"`
	AsOf            string  `json:"asOf"`
}
type WebsiteRepository interface {
	ReceiveWebsiteInquiry(context.Context, WebsiteInstallation, WebsiteInquiry, []byte) (WebsiteReceipt, error)
	WebsiteOutcomes(context.Context, WebsiteInstallation, string, string) (WebsiteOutcomes, error)
}
type WebsiteService struct {
	installations []WebsiteInstallation
	repo          WebsiteRepository
}

// Installation credentials grant one direction for one website/revision and
// environment. They never grant company export, provider access or org choice.
func OpenWebsiteService(path string, repo WebsiteRepository) (*WebsiteService, error) {
	s := &WebsiteService{repo: repo}
	if path == "" {
		return s, nil
	}
	raw, err := readWebsiteSecret(path, 65536)
	if err != nil {
		return nil, ErrWebsiteInvalid
	}
	var input struct {
		Version       int                   `json:"version"`
		Installations []WebsiteInstallation `json:"installations"`
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.DisallowUnknownFields()
	if d.Decode(&input) != nil || d.Decode(new(any)) != io.EOF || input.Version != 1 || len(input.Installations) > 64 {
		return nil, ErrWebsiteInvalid
	}
	seen := map[string]bool{}
	for _, b := range input.Installations {
		u, e := url.Parse(b.Site)
		if e != nil || u.Scheme != "https" || u.User != nil || u.Port() != "" || u.RawQuery != "" || u.Fragment != "" || u.Path != "/" || u.Host == "" || strings.ToLower(u.Host) != u.Host || !websiteID.MatchString(b.WebsiteID) || !organizationID.MatchString(b.OrganizationID) || b.WebsiteRevision < 1 || (b.Environment != "development" && b.Environment != "production") {
			return nil, ErrWebsiteInvalid
		}
		for _, name := range []string{b.WriteTokenFile, b.ReadTokenFile} {
			if filepath.Base(name) != name || name == "." || name == "" {
				return nil, ErrWebsiteInvalid
			}
		}
		write, e := readWebsiteSecret(filepath.Join(filepath.Dir(path), b.WriteTokenFile), 512)
		if e != nil {
			return nil, ErrWebsiteInvalid
		}
		read, e := readWebsiteSecret(filepath.Join(filepath.Dir(path), b.ReadTokenFile), 512)
		if e != nil {
			return nil, ErrWebsiteInvalid
		}
		b.writeToken = strings.TrimSpace(string(write))
		b.readToken = strings.TrimSpace(string(read))
		if !protectedToken.MatchString(b.writeToken) || !protectedToken.MatchString(b.readToken) || b.writeToken == b.readToken || seen[b.writeToken] || seen[b.readToken] {
			return nil, ErrWebsiteInvalid
		}
		seen[b.writeToken] = true
		seen[b.readToken] = true
		s.installations = append(s.installations, b)
	}
	return s, nil
}
func readWebsiteSecret(path string, limit int64) ([]byte, error) {
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm()&0077 != 0 || info.Size() > limit {
		return nil, ErrWebsiteInvalid
	}
	f, err := os.Open(path)
	if err != nil {
		return nil, ErrWebsiteInvalid
	}
	defer f.Close()
	opened, err := f.Stat()
	if err != nil || !os.SameFile(info, opened) {
		return nil, ErrWebsiteInvalid
	}
	raw, err := io.ReadAll(io.LimitReader(f, limit+1))
	if err != nil || int64(len(raw)) > limit {
		return nil, ErrWebsiteInvalid
	}
	return raw, nil
}
func (s *WebsiteService) Authorize(token string, write bool) (WebsiteInstallation, bool) {
	if s == nil || s.repo == nil || !protectedToken.MatchString(token) {
		return WebsiteInstallation{}, false
	}
	for _, b := range s.installations {
		want := b.readToken
		if write {
			want = b.writeToken
		}
		if subtle.ConstantTimeCompare([]byte(want), []byte(token)) == 1 {
			return b, true
		}
	}
	return WebsiteInstallation{}, false
}
func validInquiry(v WebsiteInquiry) bool {
	if !submissionID.MatchString(v.SubmissionID) {
		return false
	}
	for _, v := range []struct {
		v        string
		min, max int
	}{{v.Name, 1, 200}, {v.Email, 3, 254}, {v.Company, 0, 200}, {v.ProjectType, 0, 100}, {v.Budget, 0, 100}, {v.Message, 10, 5000}, {v.Source, 1, 100}, {v.Locale, 0, 8}} {
		if !utf8.ValidString(v.v) || len(v.v) < v.min || len(v.v) > v.max || strings.ContainsFunc(v.v, func(r rune) bool { return unicode.IsControl(r) && r != '\n' && r != '\r' && r != '\t' }) {
			return false
		}
	}
	email, err := mail.ParseAddress(v.Email)
	return err == nil && email.Address == v.Email
}
func (s *WebsiteService) Receive(ctx context.Context, b WebsiteInstallation, v WebsiteInquiry) (WebsiteReceipt, error) {
	if !validInquiry(v) {
		return WebsiteReceipt{}, ErrWebsiteInvalid
	}
	raw, _ := json.Marshal(v)
	mac := hmac.New(sha256.New, []byte(b.writeToken))
	_, _ = mac.Write(raw)
	return s.repo.ReceiveWebsiteInquiry(ctx, b, v, mac.Sum(nil))
}
func (s *WebsiteService) Report(ctx context.Context, b WebsiteInstallation, start, end string) (WebsiteOutcomes, error) {
	a, ea := time.Parse("2006-01-02", start)
	z, ez := time.Parse("2006-01-02", end)
	if ea != nil || ez != nil || z.Before(a) || z.Sub(a) > 366*24*time.Hour || z.After(time.Now().UTC().Add(24*time.Hour)) {
		return WebsiteOutcomes{}, ErrWebsiteInvalid
	}
	return s.repo.WebsiteOutcomes(ctx, b, start, end)
}
