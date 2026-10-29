package rawstore

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

var guardNow = time.Date(2026, time.October, 9, 12, 0, 0, 0, time.UTC)

// guardNotice returns a notice that passes every check, so each test breaks one field.
func guardNotice(id string, body []byte) doffin.Notice {
	sum := sha256.Sum256(body)
	return doffin.Notice{
		ID:          id,
		ContentType: "application/json",
		Body:        body,
		SHA256:      hex.EncodeToString(sum[:]),
		FetchedAt:   guardNow,
	}
}

// New(nil) has no pool. Any SQL would panic, so each refusal also proves that
// the store refused before it reached the database.

// T8: a caller's hash is never trusted; the store recomputes it from the bytes.
func TestNoticeHashIsRecomputed(t *testing.T) {
	t.Parallel()

	n := guardNotice("2025-104837", []byte(`{"id":"2025-104837"}`))
	n.SHA256 = strings.Repeat("0", 64) // well formed, but not the hash of Body

	_, err := New(nil).StoreNotice(context.Background(), 1, n)
	if !errors.Is(err, ErrInvalidNotice) {
		t.Fatalf("StoreNotice with a wrong hash: err = %v, want ErrInvalidNotice", err)
	}
}

// T9: the id shape is the one the client accepts, so a store row cannot hold an
// id that the client would refuse to download.
func TestNoticeIDShapeIsEnforced(t *testing.T) {
	t.Parallel()

	ids := []string{"2025-1", "2025-12345678901", "2025-104837\n", "../x", ""}
	for _, id := range ids {
		t.Run(fmt.Sprintf("%q", id), func(t *testing.T) {
			t.Parallel()

			_, err := New(nil).StoreNotice(context.Background(), 1, guardNotice(id, []byte("body")))
			if !errors.Is(err, ErrInvalidNotice) {
				t.Fatalf("StoreNotice(%q): err = %v, want ErrInvalidNotice", id, err)
			}
		})
	}
}

// T10: the same rules as the SQL checks. An empty body and an uppercase hex
// digest are refused, because the database would refuse them too.
func TestNoticeRejectsEmptyBodyAndUppercaseHash(t *testing.T) {
	t.Parallel()

	empty := guardNotice("2025-104837", nil)

	upper := guardNotice("2025-104837", []byte("a notice body"))
	upper.SHA256 = strings.ToUpper(upper.SHA256)

	tests := []struct {
		name string
		n    doffin.Notice
	}{
		{"empty body", empty},
		{"uppercase hex digest", upper},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			_, err := New(nil).StoreNotice(context.Background(), 1, tc.n)
			if !errors.Is(err, ErrInvalidNotice) {
				t.Fatalf("StoreNotice (%s): err = %v, want ErrInvalidNotice", tc.name, err)
			}
		})
	}
}

// A notice without its fetch time has no evidence of when it was received.
func TestNoticeNeedsItsFetchTime(t *testing.T) {
	t.Parallel()

	n := guardNotice("2025-104837", []byte("a notice body"))
	n.FetchedAt = time.Time{}

	_, err := New(nil).StoreNotice(context.Background(), 1, n)
	if !errors.Is(err, ErrInvalidNotice) {
		t.Fatalf("StoreNotice without a fetch time: err = %v, want ErrInvalidNotice", err)
	}
}
