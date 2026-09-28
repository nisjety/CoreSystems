package redact

import (
	"reflect"
	"strings"
	"testing"
)

// The String tests below moved here verbatim from internal/snapshot's
// exclude_test.go when the pattern set moved: they test the patterns, and
// the patterns now live in this package. snapshot keeps its own tests for
// what it still owns (path exclusion, and that ExcludeCredentials actually
// calls this).

func TestStringRedactsBearerAndJWT(t *testing.T) {
	t.Parallel()
	out := String("Authorization: Bearer eyJhbGciOi.eyJzdWIi.sig and sk-abcdef1234567890XYZ")
	if !strings.Contains(out, Redacted) {
		t.Fatalf("expected redaction marker, got %q", out)
	}
	if strings.Contains(out, "eyJhbGciOi") {
		t.Fatalf("JWT leaked: %q", out)
	}
	if strings.Contains(out, "sk-abcdef") {
		t.Fatalf("sk- token leaked: %q", out)
	}
}

func TestStringRedactsConnectionStringPasswordKeepsHost(t *testing.T) {
	t.Parallel()
	// Valid DSNs URL-encode reserved characters, so the password has no raw '@'.
	out := String("postgres://svc_user:s3cr3tPass@db.internal:5432/app")
	if strings.Contains(out, "s3cr3tPass") {
		t.Fatalf("password leaked: %q", out)
	}
	if !strings.Contains(out, "postgres://svc_user:") || !strings.Contains(out, "@db.internal:5432/app") {
		t.Fatalf("scheme/user/host should be preserved for diagnosability: %q", out)
	}
}

func TestStringRedactsInlineAssignmentsKeepsKey(t *testing.T) {
	t.Parallel()
	fakePassword := strings.Repeat("h", 12)
	fakeAPIKey := strings.Repeat("k", 12)
	out := String(`export DB_PASSWORD=` + fakePassword + ` && api_key: "` + fakeAPIKey + `"`)
	if strings.Contains(out, fakePassword) || strings.Contains(out, fakeAPIKey) {
		t.Fatalf("value leaked: %q", out)
	}
	if !strings.Contains(out, "DB_PASSWORD=") || !strings.Contains(out, "api_key") {
		t.Fatalf("key names should be preserved: %q", out)
	}
}

func TestStringRedactsSlackAndGoogleKeys(t *testing.T) {
	t.Parallel()
	fakeSlackToken := "xoxb-" + strings.Repeat("1", 15)
	fakeGoogleKey := "AIza" + strings.Repeat("z", 35)
	out := String("tok=" + fakeSlackToken + " key=" + fakeGoogleKey)
	if strings.Contains(out, fakeSlackToken) {
		t.Fatalf("slack token leaked: %q", out)
	}
	if strings.Contains(out, fakeGoogleKey) {
		t.Fatalf("google api key leaked: %q", out)
	}
}

func TestStringPreservesOrdinaryAssignments(t *testing.T) {
	t.Parallel()
	out := String("status=completed path=/usr/bin count=42")
	if out != "status=completed path=/usr/bin count=42" {
		t.Fatalf("non-secret assignments must survive untouched, got %q", out)
	}
}

// TestCommandRedactsASeparatedFlagValue covers the rule that exists only
// here: no text pattern can reach `--token abc123`, because every one of
// them keys on a ':' or '=' separator.
func TestCommandRedactsASeparatedFlagValue(t *testing.T) {
	t.Parallel()
	secret := strings.Repeat("s", 20)
	program, args := Command("curl", []string{"--token", secret, "https://api.example.com"})
	if program != "curl" {
		t.Fatalf("program = %q, want curl", program)
	}
	want := []string{"--token", Redacted, "https://api.example.com"}
	if !reflect.DeepEqual(args, want) {
		t.Fatalf("args = %#v, want %#v", args, want)
	}
}

func TestCommandRedactsEveryRecognizedFlagSpelling(t *testing.T) {
	t.Parallel()
	secret := strings.Repeat("s", 20)
	for _, flag := range []string{"--token", "--password", "-p", "--api-key", "--API_KEY", "--access-key", "--auth", "--client-secret", "-private-key"} {
		_, args := Command("tool", []string{flag, secret})
		if args[1] != Redacted {
			t.Fatalf("value after %q was not redacted: %#v", flag, args)
		}
	}
}

// TestCommandLeavesTheJoinedFormToStringsPatterns proves the two rules
// compose rather than overlap: `--token=x` never reaches the positional
// rule (isSecretFlag rejects anything containing '='), and is redacted by
// the inline capture pattern instead.
func TestCommandLeavesTheJoinedFormToStringsPatterns(t *testing.T) {
	t.Parallel()
	secret := strings.Repeat("s", 20)
	_, args := Command("tool", []string{"--token=" + secret})
	if strings.Contains(args[0], secret) {
		t.Fatalf("joined flag value leaked: %#v", args)
	}
	if !strings.HasPrefix(args[0], "--token=") {
		t.Fatalf("flag name should survive: %#v", args)
	}
}

func TestCommandDoesNotRedactTheNextFlag(t *testing.T) {
	t.Parallel()
	// `--token --verbose` is a missing value, not a secret named --verbose.
	_, args := Command("tool", []string{"--token", "--verbose"})
	want := []string{"--token", "--verbose"}
	if !reflect.DeepEqual(args, want) {
		t.Fatalf("args = %#v, want %#v", args, want)
	}
}

func TestCommandLeavesOrdinaryArgumentsAlone(t *testing.T) {
	t.Parallel()
	// --key is deliberately NOT a secret flag: it is a file path far more
	// often than a secret, and redacting it would teach readers to distrust
	// the redaction. See secretFlags' own comment.
	program, args := Command("/usr/bin/python3", []string{"-u", "main.py", "--key", "/etc/ssl/app.pem", "--rows", "500"})
	if program != "/usr/bin/python3" {
		t.Fatalf("program = %q", program)
	}
	want := []string{"-u", "main.py", "--key", "/etc/ssl/app.pem", "--rows", "500"}
	if !reflect.DeepEqual(args, want) {
		t.Fatalf("args = %#v, want %#v", args, want)
	}
}

func TestCommandDoesNotMutateTheCallersSlice(t *testing.T) {
	t.Parallel()
	// The unredacted argv is still needed to spawn the process, so Command
	// must copy rather than redact in place.
	secret := strings.Repeat("s", 20)
	original := []string{"--password", secret}
	_, args := Command("tool", original)
	if original[1] != secret {
		t.Fatalf("caller's slice was mutated: %#v", original)
	}
	if args[1] != Redacted {
		t.Fatalf("returned args were not redacted: %#v", args)
	}
}

func TestCommandHandlesATrailingSecretFlag(t *testing.T) {
	t.Parallel()
	// No value to redact, and no panic reading past the end.
	_, args := Command("tool", []string{"run", "--token"})
	if !reflect.DeepEqual(args, []string{"run", "--token"}) {
		t.Fatalf("args = %#v", args)
	}
}
