package trends

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"sync"
	"time"
)

const autoBudget = 8 << 20

var autoID = regexp.MustCompile(`^web_[a-f0-9]{32}$`)
var autoOrg = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$`)
var autoHash = regexp.MustCompile(`^[a-f0-9]{64}$`)

type autoDisk struct {
	Version        int         `json:"version"`
	OrganizationID string      `json:"organizationId"`
	Entries        []autoEntry `json:"entries"`
}
type autoRepository struct {
	mu        sync.Mutex
	path, org string
	state     autoDisk
}

func autoClone[T any](value T) T {
	raw, _ := json.Marshal(value)
	var next T
	_ = json.Unmarshal(raw, &next)
	return next
}
func privateAutoDir(path string) bool {
	i, e := os.Lstat(path)
	return e == nil && i.IsDir() && i.Mode().Perm() == 0700
}
func openAutomatic(path, org string) (*autoRepository, error) {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path || !autoOrg.MatchString(org) || !privateAutoDir(filepath.Dir(path)) {
		return nil, ErrInvalid
	}
	r := &autoRepository{path: path, org: org, state: autoDisk{Version: 1, OrganizationID: org, Entries: []autoEntry{}}}
	info, err := os.Lstat(path)
	if os.IsNotExist(err) {
		if err = r.persist(r.state); err != nil {
			return nil, err
		}
		return r, nil
	}
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm() != 0600 || info.Size() > autoBudget {
		return nil, ErrInvalid
	}
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	actual, err := f.Stat()
	if err != nil || !os.SameFile(info, actual) {
		return nil, ErrInvalid
	}
	raw, err := io.ReadAll(io.LimitReader(f, autoBudget+1))
	if err != nil || len(raw) > autoBudget {
		return nil, ErrInvalid
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if decoder.Decode(&r.state) != nil || decoder.Decode(new(any)) != io.EOF || r.state.Version != 1 || r.state.OrganizationID != org || r.state.Entries == nil || len(r.state.Entries) > 64 {
		return nil, ErrInvalid
	}
	seen := map[string]bool{}
	for _, entry := range r.state.Entries {
		if !validAutoEntry(entry) || seen[entry.WebsiteID] {
			return nil, ErrInvalid
		}
		seen[entry.WebsiteID] = true
	}
	return r, nil
}
func validAutoEntry(e autoEntry) bool {
	canonical, err := pulse.CanonicalSite(e.Site)
	if err != nil || canonical != e.Site {
		return false
	}
	if !autoID.MatchString(e.WebsiteID) || e.WebsiteRevision < 1 || e.Source != "marketing_trends_auto" || timeValue(e.UpdatedAt).IsZero() || timeValue(e.NextSyncAt).IsZero() {
		return false
	}
	switch e.Status {
	case "pending", "waiting_for_search_console", "waiting_for_keywords", "running", "complete", "failed":
	default:
		return false
	}
	if e.Seed != nil {
		start, a := time.Parse(time.DateOnly, e.Seed.Period.StartDate)
		end, b := time.Parse(time.DateOnly, e.Seed.Period.EndDate)
		if e.Seed.Source != "google_search_console" || e.Seed.Selection != "impressions_then_clicks" || !Valid(Query{Terms: e.Seed.Terms, Timeframe: "today 12-m"}) || timeValue(e.Seed.ObservedAt).IsZero() || a != nil || b != nil || end.Before(start) {
			return false
		}
		if !autoHash.MatchString(e.SeedKey) {
			return false
		}
	} else if e.SeedKey != "" || e.Report != nil {
		return false
	}
	if e.Status == "running" || e.Status == "complete" {
		if e.Seed == nil {
			return false
		}
	}
	if e.Status == "complete" && (e.Report == nil || (e.Report.Status != "available" && e.Report.Status != "no_data")) {
		return false
	}
	if e.Report != nil {
		q := Query{Terms: e.Seed.Terms, Timeframe: "today 12-m"}
		if !ValidReport(*e.Report, q, timeValue(e.Report.ObservedAt).Add(time.Minute)) || timeValue(e.ExpiresAt).IsZero() {
			return false
		}
	}
	return true
}
func (r *autoRepository) get(id string) autoEntry {
	r.mu.Lock()
	defer r.mu.Unlock()
	for _, e := range r.state.Entries {
		if e.WebsiteID == id {
			return autoClone(e)
		}
	}
	return autoEntry{}
}
func (r *autoRepository) save(entry autoEntry) error {
	if !validAutoEntry(entry) {
		return ErrInvalid
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	next := autoClone(r.state)
	found := false
	for i, e := range next.Entries {
		if e.WebsiteID == entry.WebsiteID {
			next.Entries[i] = autoClone(entry)
			found = true
			break
		}
	}
	if !found {
		if len(next.Entries) >= 64 {
			return ErrUnavailable
		}
		next.Entries = append(next.Entries, autoClone(entry))
	}
	err := r.persist(next)
	if err == nil || errors.Is(err, errAutoCommitted) {
		r.state = next
	}
	return err
}
func (r *autoRepository) persist(state autoDisk) error {
	if !privateAutoDir(filepath.Dir(r.path)) {
		return ErrInvalid
	}
	if info, err := os.Lstat(r.path); err == nil {
		if !info.Mode().IsRegular() || info.Mode().Perm() != 0600 {
			return ErrInvalid
		}
	} else if !os.IsNotExist(err) {
		return err
	}
	raw, err := json.Marshal(state)
	if err != nil || len(raw) > autoBudget {
		return ErrInvalid
	}
	file, err := os.CreateTemp(filepath.Dir(r.path), ".trends-*")
	if err != nil {
		return err
	}
	defer os.Remove(file.Name())
	if err = file.Chmod(0600); err == nil {
		_, err = file.Write(raw)
	}
	if err == nil {
		err = file.Sync()
	}
	closeErr := file.Close()
	if err == nil {
		err = closeErr
	}
	if err != nil {
		return err
	}
	dir, err := os.Open(filepath.Dir(r.path))
	if err != nil {
		return err
	}
	defer dir.Close()
	if err = os.Rename(file.Name(), r.path); err != nil {
		return err
	}
	if err = dir.Sync(); err != nil {
		return fmt.Errorf("%w: %v", errAutoCommitted, err)
	}
	return nil
}
