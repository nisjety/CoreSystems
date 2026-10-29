package pg

import (
	"errors"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func TestCursorEncodeDecodeRoundTrip(t *testing.T) {
	t.Parallel()

	encoded := encodeCursor(12345, "job_01")
	decoded, err := decodeCursor(encoded)
	if err != nil {
		t.Fatalf("decodeCursor error: %v", err)
	}
	if decoded == nil {
		t.Fatal("decoded cursor is nil")
	}
	if decoded.CreatedAt != 12345 || decoded.ID != "job_01" {
		t.Fatalf("decoded=%+v want created_at=12345 id=job_01", decoded)
	}
}

func TestDecodeCursorErrors(t *testing.T) {
	t.Parallel()

	t.Run("empty cursor", func(t *testing.T) {
		t.Parallel()
		c, err := decodeCursor("")
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if c != nil {
			t.Fatalf("expected nil cursor, got %+v", c)
		}
	})

	t.Run("invalid base64", func(t *testing.T) {
		t.Parallel()
		if _, err := decodeCursor("*"); err == nil {
			t.Fatal("expected error for invalid base64")
		}
	})

	t.Run("malformed payload", func(t *testing.T) {
		t.Parallel()
		if _, err := decodeCursor("YWJj"); err == nil { // "abc"
			t.Fatal("expected malformed cursor error")
		}
	})

	t.Run("invalid timestamp", func(t *testing.T) {
		t.Parallel()
		bad := encodeCursor(1, "x")
		// Replace encoded payload with "nan|x"
		bad = "bmFufHg"
		if _, err := decodeCursor(bad); err == nil {
			t.Fatal("expected timestamp parse error")
		}
	})
}

func TestPageLimit(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name   string
		limit  int
		max    int
		wanted int
	}{
		{name: "normal", limit: 10, max: 100, wanted: 10},
		{name: "zero uses max", limit: 0, max: 100, wanted: 100},
		{name: "negative uses max", limit: -1, max: 100, wanted: 100},
		{name: "over max uses max", limit: 101, max: 100, wanted: 100},
	}

	for _, tt := range tests {
		tt := tt
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if got := pageLimit(tt.limit, tt.max); got != tt.wanted {
				t.Fatalf("pageLimit(%d,%d)=%d want=%d", tt.limit, tt.max, got, tt.wanted)
			}
		})
	}
}

func TestMapPgErr(t *testing.T) {
	t.Parallel()

	if got := mapPgErr(nil); got != nil {
		t.Fatalf("mapPgErr(nil)=%v want=nil", got)
	}
	if got := mapPgErr(pgx.ErrNoRows); !errors.Is(got, store.ErrNotFound) {
		t.Fatalf("mapPgErr(ErrNoRows)=%v want ErrNotFound", got)
	}
	if got := mapPgErr(&pgconn.PgError{Code: "23505"}); !errors.Is(got, store.ErrConflict) {
		t.Fatalf("mapPgErr(23505)=%v want ErrConflict", got)
	}

	other := errors.New("boom")
	if got := mapPgErr(other); !errors.Is(got, other) {
		t.Fatalf("mapPgErr(other)=%v want original error", got)
	}
}
