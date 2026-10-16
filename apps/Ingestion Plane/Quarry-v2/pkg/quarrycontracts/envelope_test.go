package quarrycontracts

import (
	"os"
	"regexp"
	"sort"
	"strings"
	"testing"
)

// goCodes is every Go constant with the HTTP status Rust gives the same code
// (crates/quarry-core/src/error.rs `http_status`). A code missing from
// HTTPStatus() falls through to 500, so each one is listed explicitly.
var goCodes = map[ErrorCode]int{
	CodeBadRequest:           400,
	CodeUnauthorized:         401,
	CodeForbidden:            403,
	CodeNotFound:             404,
	CodeConflict:             409,
	CodeRateLimited:          429,
	CodeTimeout:              504,
	CodeSecurityBlocked:      403,
	CodeActionUnknown:        409,
	CodeChallengeDetected:    403,
	CodeRuntimeNotReady:      503,
	CodeCheckpointLost:       409,
	CodeTargetRepairRequired: 409,
	CodeDriverFailed:         502,
	CodeUpstreamBlocked:      502,
	CodeUnsupported:          501,
	CodeUnsupportedMedia:     415,
	CodeBudgetExhausted:      402,
	CodeInternal:             500,
}

func TestHTTPStatusIsExplicitForEveryCode(t *testing.T) {
	for code, want := range goCodes {
		if got := code.HTTPStatus(); got != want {
			t.Errorf("%s: HTTPStatus() = %d, want %d", code, got, want)
		}
	}
	// An unknown code is an internal fault, never a silent 200-class status.
	if got := ErrorCode("NOT_A_CODE").HTTPStatus(); got != 500 {
		t.Errorf("unknown code status = %d, want 500", got)
	}
}

// TestErrorCodeMirrorsTheRustEnum reads the Rust enum and fails when a variant
// has no Go constant in goCodes, or the other way round, so adding a code to
// one language cannot silently turn into a 500 in the other.
func TestErrorCodeMirrorsTheRustEnum(t *testing.T) {
	source, err := os.ReadFile("../../crates/quarry-core/src/error.rs")
	if err != nil {
		t.Skipf("Rust source not available from this module: %v", err)
	}
	text := strings.ReplaceAll(string(source), "\r\n", "\n")
	start := strings.Index(text, "pub enum ErrorCode {")
	if start < 0 {
		t.Fatal("ErrorCode enum not found in error.rs")
	}
	body := text[start+len("pub enum ErrorCode {"):]
	body = body[:strings.Index(body, "\n}")]

	variant := regexp.MustCompile(`^\s{4}([A-Z][A-Za-z]+),\s*$`)
	var rust []string
	for _, line := range strings.Split(body, "\n") {
		if m := variant.FindStringSubmatch(line); m != nil {
			rust = append(rust, screamingSnake(m[1]))
		}
	}
	if len(rust) < 15 {
		t.Fatalf("parsed only %d Rust variants: %v", len(rust), rust)
	}

	var gocodes []string
	for code := range goCodes {
		gocodes = append(gocodes, string(code))
	}
	sort.Strings(rust)
	sort.Strings(gocodes)
	if strings.Join(rust, ",") != strings.Join(gocodes, ",") {
		t.Fatalf("Rust and Go error codes differ:\n rust: %v\n   go: %v", rust, gocodes)
	}
}

func screamingSnake(name string) string {
	var out []rune
	for i, r := range name {
		if i > 0 && r >= 'A' && r <= 'Z' {
			out = append(out, '_')
		}
		out = append(out, r)
	}
	return strings.ToUpper(string(out))
}
