package websites

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sync"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

const maxWebsites = 64
const maxRegistryBytes = 1 << 20

var errCommitted = errors.New("website update committed but directory synchronization failed")

type diskRegistry struct {
	Version        int      `json:"version"`
	OrganizationID string   `json:"organizationId"`
	Records        []Record `json:"records"`
}

// Repository is single-process/single-writer. Its private volume must never be
// mounted by a second Marketing replica. Updates replace, sync, then publish.
type Repository struct {
	mu        sync.Mutex
	path, org string
	records   []Record
}

func Open(path, org string) (*Repository, error) {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path || !identityPattern.MatchString(org) {
		return nil, ErrInvalid
	}
	dir, err := os.Lstat(filepath.Dir(path))
	if err != nil || !dir.IsDir() || dir.Mode().Perm() != 0700 {
		return nil, fmt.Errorf("website registry requires a private mode-0700 directory")
	}
	repo := &Repository{path: path, org: org, records: []Record{}}
	info, err := os.Lstat(path)
	if os.IsNotExist(err) {
		if err := repo.persist(repo.records); err != nil {
			return nil, err
		}
		return repo, nil
	}
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm() != 0600 || info.Size() > maxRegistryBytes {
		return nil, fmt.Errorf("website registry must be a protected regular file")
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	current, err := file.Stat()
	if err != nil || !os.SameFile(info, current) {
		return nil, ErrInvalid
	}
	raw, err := io.ReadAll(io.LimitReader(file, maxRegistryBytes+1))
	if err != nil || len(raw) > maxRegistryBytes {
		return nil, ErrInvalid
	}
	var state diskRegistry
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if decoder.Decode(&state) != nil || decoder.Decode(new(any)) != io.EOF || state.Version != 1 || state.OrganizationID != org || validateRecords(state.Records) != nil {
		return nil, fmt.Errorf("website registry is corrupt or belongs to another organization")
	}
	repo.records = copyRecords(state.Records)
	return repo, nil
}

func (r *Repository) List() []Record { r.mu.Lock(); defer r.mu.Unlock(); return copyRecords(r.records) }
func (r *Repository) Update(fn func([]Record) ([]Record, error)) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	next, err := fn(copyRecords(r.records))
	if err != nil {
		return err
	}
	if err := validateRecords(next); err != nil {
		return err
	}
	err = r.persist(next)
	if err == nil || errors.Is(err, errCommitted) {
		r.records = copyRecords(next)
	}
	return err
}
func copyRecords(records []Record) []Record {
	result := make([]Record, len(records))
	for i, record := range records {
		result[i] = record
		if record.DNS != nil {
			challenge := *record.DNS
			result[i].DNS = &challenge
		}
	}
	return result
}
func validateRecords(records []Record) error {
	if records == nil || len(records) > maxWebsites {
		return ErrInvalid
	}
	ids, sites := map[string]bool{}, map[string]bool{}
	for _, record := range records {
		if record.GSCChoiceID != "" && !resourcePattern.MatchString(record.GSCChoiceID) || record.GA4ChoiceID != "" && !resourcePattern.MatchString(record.GA4ChoiceID) {
			return ErrInvalid
		}
		if record.ConnectionsFingerprint != "" && !connectionsFingerprintPattern.MatchString(record.ConnectionsFingerprint) {
			return ErrInvalid
		}
		w := record.Website
		site, err := pulse.CanonicalSite(w.Site)
		if err != nil || site != w.Site || !idPattern.MatchString(w.ID) || ids[w.ID] || sites[site] || w.Revision < 1 || !timestamp(w.CreatedAt) || !timestamp(w.UpdatedAt) {
			return ErrInvalid
		}
		if !validOwnership(w.Ownership) || !validMappings(record) {
			return ErrInvalid
		}
		if record.DNS != nil && (!timestamp(record.DNS.ExpiresAt) || record.DNS.Name != dnsName(w.Site) || !dnsValuePattern.MatchString(record.DNS.Value)) {
			return ErrInvalid
		}
		ids[w.ID], sites[site] = true, true
	}
	return nil
}
func (r *Repository) persist(records []Record) error {
	raw, err := json.Marshal(diskRegistry{Version: 1, OrganizationID: r.org, Records: records})
	if err != nil || len(raw) > maxRegistryBytes {
		return ErrInvalid
	}
	file, err := os.CreateTemp(filepath.Dir(r.path), ".websites-*")
	if err != nil {
		return fmt.Errorf("create website registry transaction: %w", err)
	}
	name := file.Name()
	defer os.Remove(name)
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
		return fmt.Errorf("write website registry: %w", err)
	}
	dir, err := os.Open(filepath.Dir(r.path))
	if err != nil {
		return err
	}
	defer dir.Close()
	if err = os.Rename(name, r.path); err != nil {
		return fmt.Errorf("replace website registry: %w", err)
	}
	if err := dir.Sync(); err != nil {
		return fmt.Errorf("%w: %v", errCommitted, err)
	}
	return nil
}
