package doffin

import (
	"context"
	"errors"
	"testing"
)

// IsNoticeID must accept and refuse the same ids as the pattern Download uses.
// Refused ids are also refused by Download before any call, so the table is
// checked against both.
func TestIsNoticeIDAgreesWithTheClientPattern(t *testing.T) {
	t.Parallel()

	tests := []struct {
		id   string
		want bool
	}{
		{"2025-1234", true},
		{"2025-104837", true},
		{"2025-123456789", true},
		{"2025-1", false},
		{"2025-12345678901", false},
		{"2025-104837\n", false},
		{" 2025-104837", false},
		{"2025-104837 ", false},
		{"../x", false},
		{"2025-10483a", false},
		{"", false},
	}
	for _, tc := range tests {
		t.Run(tc.id, func(t *testing.T) {
			t.Parallel()

			if got := IsNoticeID(tc.id); got != tc.want {
				t.Fatalf("IsNoticeID(%q) = %v, want %v", tc.id, got, tc.want)
			}
			if got := noticeIDPattern.MatchString(tc.id); got != tc.want {
				t.Fatalf("noticeIDPattern(%q) = %v, want %v: the two checks disagree", tc.id, got, tc.want)
			}
			if !tc.want {
				// A zero client has no transport. Download refuses the id before any call.
				_, err := (&Client{}).Download(context.Background(), tc.id)
				if !errors.Is(err, ErrInvalidParams) {
					t.Fatalf("Download(%q) error = %v, want ErrInvalidParams before any call", tc.id, err)
				}
			}
		})
	}
}
