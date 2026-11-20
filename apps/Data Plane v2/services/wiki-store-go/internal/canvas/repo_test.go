package canvas

import (
	"encoding/json"
	"errors"
	"os"
	"strings"
	"testing"
)

// canvasGoldenPath is Control's shared canvas decision vector (WS19-S2), read-only from this package.
const canvasGoldenPath = "../../../../../Control Plane/user-core/internal/spaces/testdata/space-canvas-decision.golden.json"

// requireCanvasError fails unless err is a typed *Error carrying the wire code, and returns it.
func requireCanvasError(t *testing.T, err error, code string) *Error {
	t.Helper()
	var typed *Error
	if !errors.As(err, &typed) {
		t.Fatalf("want a %s error, got %v", code, err)
	}
	if typed.Code != code {
		t.Fatalf("error code = %s, want %s (%s)", typed.Code, code, typed.Message)
	}
	return typed
}

func TestValidateContentBoundsAndEncoding(t *testing.T) {
	for name, content := range map[string]string{
		"one byte":         "x",
		"the byte limit":   strings.Repeat("x", MaxContentBytes),
		"multi-byte UTF-8": "héllo room",
	} {
		if err := ValidateContent(content); err != nil {
			t.Errorf("%s was refused: %v", name, err)
		}
	}

	typed := requireCanvasError(t, ValidateContent(strings.Repeat("x", MaxContentBytes+1)), CodeTooLarge)
	if typed.MaxBytes != MaxContentBytes {
		t.Errorf("canvas_too_large MaxBytes = %d, want %d", typed.MaxBytes, MaxContentBytes)
	}

	for name, content := range map[string]string{
		"empty":         "",
		"invalid UTF-8": "\xff",
		"NUL byte":      "a\x00b",
	} {
		t.Run(name, func(t *testing.T) {
			requireCanvasError(t, ValidateContent(content), CodeInvalidRequest)
		})
	}
}

// TestValidateContentAcceptsAOneCharacterClear (D22aq): a canvas is cleared by saving a one-character document, so every
// one-character document is valid content, whitespace and multi-byte characters included. The minimum still refuses empty
// content, and a lone NUL is still refused.
func TestValidateContentAcceptsAOneCharacterClear(t *testing.T) {
	cases := map[string]struct {
		content string
		ok      bool
	}{
		"a newline":                    {"\n", true},
		"a space":                      {" ", true},
		"a hyphen":                     {"-", true},
		"a two-byte character":         {"é", true},
		"a four-byte character":        {"\U0001F5D2", true},
		"empty":                        {"", false},
		"a NUL byte":                   {"\x00", false},
		"half of a two-byte character": {"\xc3", false},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			err := ValidateContent(tc.content)
			if tc.ok {
				if err != nil {
					t.Fatalf("a one-character clear %q was refused: %v", tc.content, err)
				}
				return
			}
			requireCanvasError(t, err, CodeInvalidRequest)
		})
	}
	if MinContentBytes != 1 {
		t.Fatalf("MinContentBytes = %d, want 1: no empty version exists, and a one-character clear must pass", MinContentBytes)
	}
}

func TestNormalizeSummaryCountsCharactersAndBlankBecomesNil(t *testing.T) {
	// 200 characters are 400 bytes here, so a byte count would refuse them. The limit counts characters.
	two := strings.Repeat("é", MaxSummaryChars)
	got, err := NormalizeSummary(&two)
	if err != nil || got == nil || *got != two {
		t.Fatalf("a 200-character summary was not kept: got %v, err %v", got, err)
	}

	tooLong := strings.Repeat("é", MaxSummaryChars+1)
	_, err = NormalizeSummary(&tooLong)
	requireCanvasError(t, err, CodeInvalidRequest)

	for name, summary := range map[string]*string{"blank": ptr("   \t "), "absent": nil} {
		got, err := NormalizeSummary(summary)
		if err != nil || got != nil {
			t.Errorf("%s summary should become nil, got %v, err %v", name, got, err)
		}
	}
}

// TestContentDigestMatchesTheControlGoldenHex checks the content digest against the hex in Control's shared vector. The
// canvas issuer and this store must agree on it, or every decision fails its digest check.
func TestContentDigestMatchesTheControlGoldenHex(t *testing.T) {
	raw, err := os.ReadFile(canvasGoldenPath)
	if err != nil {
		t.Fatalf("read the Control canvas golden: %v", err)
	}
	var golden struct {
		Vectors []struct {
			Name          string  `json:"name"`
			Content       *string `json:"content"`
			ContentDigest string  `json:"content_digest"`
		} `json:"vectors"`
	}
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatalf("decode the Control canvas golden: %v", err)
	}
	checked := 0
	for _, vector := range golden.Vectors {
		if vector.Content == nil {
			continue
		}
		checked++
		if got := ContentDigest(*vector.Content); got != vector.ContentDigest {
			t.Errorf("%s: ContentDigest = %s, golden %s", vector.Name, got, vector.ContentDigest)
		}
	}
	if checked == 0 {
		t.Fatal("the golden carries no content vector to check the digest against")
	}
}

func TestIdempotencyHashBindsTheOperationAndLengthPrefixesEachField(t *testing.T) {
	author := Author{Kind: AuthorUser, ID: "user-2"}
	base := idempotencyHash("org-1", "canvas-1", opSave, author, "key-1")

	// Moving a boundary between two fields must change the hash. A plain concatenation would not notice, because each pair
	// below joins to the same bytes as the base tuple.
	if idempotencyHash("org-1c", "anvas-1", opSave, author, "key-1") == base {
		t.Error("shifting a boundary between org and canvas kept the same hash")
	}
	if idempotencyHash("org-1", "canvas-1k", opSave, author, "ey-1") == base {
		t.Error("shifting a boundary between canvas and key kept the same hash")
	}

	// One key cannot replay a save as a restore.
	if idempotencyHash("org-1", "canvas-1", opRestore, author, "key-1") == base {
		t.Error("the same key for save and restore produced the same hash")
	}
}

func TestClampListLimitBoundsThePageSize(t *testing.T) {
	cases := map[string]struct{ in, want int }{
		"unset":        {0, DefaultListLimit},
		"negative":     {-3, DefaultListLimit},
		"one":          {1, 1},
		"the maximum":  {MaxListLimit, MaxListLimit},
		"over maximum": {MaxListLimit + 1, MaxListLimit},
		"far over":     {10000, MaxListLimit},
	}
	for name, tc := range cases {
		if got := ClampListLimit(tc.in); got != tc.want {
			t.Errorf("ClampListLimit(%s = %d) = %d, want %d", name, tc.in, got, tc.want)
		}
	}
}

// TestErrorCodesAreTheWireStrings pins the typed codes the HTTP layer returns and the Space SPA and Control read by name.
func TestErrorCodesAreTheWireStrings(t *testing.T) {
	pairs := []struct{ constant, wire string }{
		{CodeCanvasNotFound, "canvas_not_found"},
		{CodeVersionNotFound, "canvas_version_not_found"},
		{CodeHeadMoved, "canvas_head_moved"},
		{CodeIdempotencyKeyConflict, "idempotency_key_conflict"},
		{CodeTooLarge, "canvas_too_large"},
		{CodeInvalidRequest, "invalid_canvas_request"},
		{CodeInvalidCursor, "invalid_cursor"},
		{CodeRateLimited, "canvas_rate_limited"},
		{CodeRunWriteLimit, "canvas_run_write_limit"},
	}
	for _, pair := range pairs {
		if pair.constant != pair.wire {
			t.Errorf("code %q is not the wire string %q", pair.constant, pair.wire)
		}
	}

	refusal := &Error{Code: CodeHeadMoved, Message: "the canvas head moved"}
	if got := refusal.Error(); got != "canvas_head_moved: the canvas head moved" {
		t.Errorf("Error() = %q", got)
	}
}

func ptr(s string) *string { return &s }
