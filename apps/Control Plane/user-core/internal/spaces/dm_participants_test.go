package spaces

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

// WS12-S1: participant-set identity. testdata/participant-set-vectors.v1.json
// is shared with Application's Convex test, so a one-sided change to the
// encoding fails on both sides.

type participantVectors struct {
	Version int    `json:"version"`
	Domain  string `json:"domain"`
	Valid   []struct {
		Name         string   `json:"name"`
		InputIDs     []string `json:"input_ids"`
		CanonicalIDs []string `json:"canonical_ids"`
		Kind         string   `json:"kind"`
		Hash         string   `json:"hash"`
	} `json:"valid"`
	Invalid []struct {
		Name     string   `json:"name"`
		InputIDs []string `json:"input_ids"`
		Reason   string   `json:"reason"`
	} `json:"invalid"`
}

func loadParticipantVectors(t *testing.T) participantVectors {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join("testdata", "participant-set-vectors.v1.json"))
	if err != nil {
		t.Fatalf("read vectors: %v", err)
	}
	var vectors participantVectors
	if err := json.Unmarshal(raw, &vectors); err != nil {
		t.Fatalf("parse vectors: %v", err)
	}
	if vectors.Version != 1 || vectors.Domain != "participant-set/v1" || len(vectors.Valid) == 0 || len(vectors.Invalid) == 0 {
		t.Fatalf("vector file is not the v1 shape: version=%d domain=%q valid=%d invalid=%d",
			vectors.Version, vectors.Domain, len(vectors.Valid), len(vectors.Invalid))
	}
	return vectors
}

func TestParticipantSetHashVectors(t *testing.T) {
	vectors := loadParticipantVectors(t)
	for _, vector := range vectors.Valid {
		t.Run(vector.Name, func(t *testing.T) {
			canonical, err := CanonicalParticipants(vector.InputIDs)
			if err != nil {
				t.Fatalf("CanonicalParticipants: %v", err)
			}
			if !reflect.DeepEqual(canonical, vector.CanonicalIDs) {
				t.Fatalf("canonical = %q, want %q", canonical, vector.CanonicalIDs)
			}
			hash, err := ParticipantSetHash(vector.InputIDs...)
			if err != nil {
				t.Fatalf("ParticipantSetHash: %v", err)
			}
			if hash != vector.Hash {
				t.Fatalf("hash = %s, want %s", hash, vector.Hash)
			}
			kind, err := DMKindForCount(len(canonical))
			if err != nil || string(kind) != vector.Kind {
				t.Fatalf("kind = %q err=%v, want %q", kind, err, vector.Kind)
			}
		})
	}
	for _, vector := range vectors.Invalid {
		t.Run("invalid "+vector.Name, func(t *testing.T) {
			_, err := ParticipantSetHash(vector.InputIDs...)
			var invalid *DMParticipantInvalidError
			if !errors.As(err, &invalid) || !errors.Is(err, ErrDMParticipantInvalid) {
				t.Fatalf("err = %v, want a DMParticipantInvalidError", err)
			}
			if invalid.Reason != vector.Reason {
				t.Fatalf("reason = %q, want %q", invalid.Reason, vector.Reason)
			}
		})
	}
}

func TestParticipantSetHashIgnoresOrderAndIsItsOwnDomain(t *testing.T) {
	a, err := ParticipantSetHash("u1", "u2", "u3")
	if err != nil {
		t.Fatal(err)
	}
	b, _ := ParticipantSetHash("u3", "u1", "u2")
	if a != b {
		t.Fatal("input order changed the hash")
	}
	other, _ := ParticipantSetHash("u1", "u2", "u4")
	if a == other {
		t.Fatal("a different set produced the same hash")
	}
	if !strings.HasPrefix(a, ParticipantHashPrefix) || len(a) != len(ParticipantHashPrefix)+64 {
		t.Fatalf("hash %q is not sha256:<64 hex>", a)
	}
	// Length prefixes keep ("ab","c") apart from ("a","bc").
	x, _ := ParticipantSetHash("ab", "c")
	y, _ := ParticipantSetHash("a", "bc")
	if x == y {
		t.Fatal("ids are not length-prefixed")
	}
}

func TestDMKindForCountBoundaries(t *testing.T) {
	for count, want := range map[int]Kind{2: KindDM, 3: KindGroupDM, 9: KindGroupDM} {
		if got, err := DMKindForCount(count); err != nil || got != want {
			t.Errorf("count %d: kind=%q err=%v, want %q", count, got, err, want)
		}
	}
	for _, count := range []int{-1, 0, 1, 10, 100} {
		_, err := DMKindForCount(count)
		var invalid *DMParticipantInvalidError
		if !errors.As(err, &invalid) || invalid.Reason != DMReasonCount {
			t.Errorf("count %d: err = %v, want reason count", count, err)
		}
	}
}

func TestDMErrorsMatchTheirSentinelsAndNameWhatIsMissing(t *testing.T) {
	notEntitled := &DMNotEntitledError{Missing: []string{"dm_create_entitled", "thread_read_entitled"}}
	if !errors.Is(notEntitled, ErrDMNotEntitled) || errors.Is(notEntitled, ErrDMParticipantInvalid) {
		t.Fatal("DMNotEntitledError must match only ErrDMNotEntitled")
	}
	if !strings.Contains(notEntitled.Error(), "dm_create_entitled,thread_read_entitled") {
		t.Fatalf("message %q does not list the missing bits", notEntitled.Error())
	}
	invalid := &DMParticipantInvalidError{Reason: DMReasonSelf}
	if !errors.Is(invalid, ErrDMParticipantInvalid) || errors.Is(invalid, ErrDMNotEntitled) || !strings.Contains(invalid.Error(), DMReasonSelf) {
		t.Fatal("DMParticipantInvalidError must match only ErrDMParticipantInvalid and carry its reason")
	}
	var wrapped error = errors.Join(errors.New("ctx"), notEntitled)
	var got *DMNotEntitledError
	if !errors.As(wrapped, &got) || len(got.Missing) != 2 {
		t.Fatal("errors.As lost the missing list")
	}
}

func TestDMRegistrationSwitchDefaultsOff(t *testing.T) {
	var nilRepo *Repository
	if nilRepo.DMRegistrationEnabled() {
		t.Fatal("a nil repository must report DM registration off")
	}
	repo := &Repository{}
	if repo.DMRegistrationEnabled() {
		t.Fatal("DM registration must default off")
	}
	repo.SetDMRegistrationEnabled(true)
	if !repo.DMRegistrationEnabled() {
		t.Fatal("the switch did not turn on")
	}
	repo.SetDMRegistrationEnabled(false)
	if repo.DMRegistrationEnabled() {
		t.Fatal("the switch did not turn off")
	}
}
