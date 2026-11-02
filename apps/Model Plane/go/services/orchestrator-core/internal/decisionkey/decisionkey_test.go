package decisionkey

import (
	"bytes"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"log/slog"
	"strings"
	"testing"
	"time"
)

func testKey(seed byte) ed25519.PublicKey {
	return ed25519.NewKeyFromSeed(bytes.Repeat([]byte{seed}, ed25519.SeedSize)).Public().(ed25519.PublicKey)
}

func testLogger(buf *bytes.Buffer) *slog.Logger {
	return slog.New(slog.NewJSONHandler(buf, nil))
}

// A key whose standard-base64 form differs from its URL-safe form, so the two
// decoders cannot be confused with each other.
func keyWithURLSpecificChars(t *testing.T) ed25519.PublicKey {
	t.Helper()
	for seed := 0; seed < 255; seed++ {
		key := testKey(byte(seed))
		if strings.ContainsAny(base64.RawStdEncoding.EncodeToString(key), "+/") {
			return key
		}
	}
	t.Fatal("no test key with +// in standard base64")
	return nil
}

func TestDecodePublicKeyPrefersURLSafeUnpadded(t *testing.T) {
	key := keyWithURLSpecificChars(t)
	got, canonical, err := DecodePublicKey(base64.RawURLEncoding.EncodeToString(key))
	if err != nil || !canonical || !bytes.Equal(got, key) {
		t.Fatalf("unpadded base64url: got canonical=%v err=%v equal=%v", canonical, err, bytes.Equal(got, key))
	}
}

func TestDecodePublicKeyAcceptsOtherEncodingsAsNonCanonical(t *testing.T) {
	key := keyWithURLSpecificChars(t)
	for name, encoded := range map[string]string{
		"padded url-safe": base64.URLEncoding.EncodeToString(key),
		"standard":        base64.StdEncoding.EncodeToString(key),
		"raw standard":    base64.RawStdEncoding.EncodeToString(key),
	} {
		got, canonical, err := DecodePublicKey("  " + encoded + "\n")
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if canonical {
			t.Fatalf("%s: reported canonical", name)
		}
		if !bytes.Equal(got, key) {
			t.Fatalf("%s: decoded to a different key", name)
		}
	}
}

func TestDecodePublicKeyRejectsBadValuesWithoutEchoingThem(t *testing.T) {
	short := base64.RawURLEncoding.EncodeToString([]byte("short-secret-material"))
	for name, raw := range map[string]string{
		"empty":      "",
		"blank":      "   ",
		"not base64": "!!!not-base64!!!",
		"wrong size": short,
	} {
		_, _, err := DecodePublicKey(raw)
		if err == nil {
			t.Fatalf("%s: expected an error", name)
		}
		if raw != "" && strings.Contains(err.Error(), strings.TrimSpace(raw)) && strings.TrimSpace(raw) != "" {
			t.Fatalf("%s: error echoes the value: %v", name, err)
		}
	}
}

func TestFingerprintMatchesSha256PrefixAndIsNotTheKey(t *testing.T) {
	key := testKey(7)
	digest := sha256.Sum256(key)
	if got, want := Fingerprint(key), hex.EncodeToString(digest[:8]); got != want {
		t.Fatalf("fingerprint %q, want %q", got, want)
	}
	if len(Fingerprint(key)) != 16 {
		t.Fatalf("fingerprint length %d", len(Fingerprint(key)))
	}
	if Fingerprint(key) == Fingerprint(testKey(8)) {
		t.Fatal("different keys share a fingerprint")
	}
	if strings.Contains(base64.RawURLEncoding.EncodeToString(key), Fingerprint(key)) {
		t.Fatal("fingerprint is a substring of the key encoding")
	}
}

func TestSanitizeKeyIDStripsControlCharsAndCapsLength(t *testing.T) {
	if got := SanitizeKeyID("a\nb\x1b[31mc"); strings.ContainsAny(got, "\n\x1b") {
		t.Fatalf("control characters survived: %q", got)
	}
	long := strings.Repeat("k", 200)
	got := SanitizeKeyID(long)
	if !strings.HasSuffix(got, "...") || len(got) != loggedKeyIDMaxChars+3 {
		t.Fatalf("long id not capped: len=%d", len(got))
	}
	if SanitizeKeyID("control-2026-10") != "control-2026-10" {
		t.Fatal("an ordinary id was altered")
	}
}

func TestLimiterAdmitsOncePerWindowPerID(t *testing.T) {
	limiter := NewLimiter(time.Minute, 4)
	start := time.Date(2026, 10, 31, 12, 0, 0, 0, time.UTC)
	if !limiter.Allow("a", start) {
		t.Fatal("first sighting must log")
	}
	if limiter.Allow("a", start.Add(59*time.Second)) {
		t.Fatal("repeat inside the window must not log")
	}
	if !limiter.Allow("b", start.Add(time.Second)) {
		t.Fatal("a different id logs independently")
	}
	if !limiter.Allow("a", start.Add(61*time.Second)) {
		t.Fatal("after the window the id logs again")
	}
}

func TestLimiterIsBoundedAgainstCallerControlledIDs(t *testing.T) {
	limiter := NewLimiter(time.Minute, 3)
	now := time.Date(2026, 10, 31, 12, 0, 0, 0, time.UTC)
	for _, id := range []string{"a", "b", "c"} {
		if !limiter.Allow(id, now) {
			t.Fatalf("%s should log", id)
		}
	}
	if limiter.Allow("d", now) {
		t.Fatal("at capacity with no stale entries, a new id must not log")
	}
	if len(limiter.last) != 3 {
		t.Fatalf("limiter grew to %d entries", len(limiter.last))
	}
	if !limiter.Allow("d", now.Add(2*time.Minute)) {
		t.Fatal("stale entries are dropped to make room")
	}
}

func TestReportUnknownKeyLogsBothIDsOnceAndNeverTheKey(t *testing.T) {
	var buf bytes.Buffer
	limiter := NewLimiter(time.Minute, 4)
	now := time.Date(2026, 10, 31, 12, 0, 0, 0, time.UTC)
	reportUnknownKey(testLogger(&buf), limiter, now, "cron", "ledger-key-2", "stale-key-1")
	reportUnknownKey(testLogger(&buf), limiter, now.Add(time.Second), "cron", "ledger-key-2", "stale-key-1")
	out := buf.String()
	if strings.Count(out, "\n") != 1 {
		t.Fatalf("want exactly one log line, got: %q", out)
	}
	for _, want := range []string{`"level":"ERROR"`, `"token_key_id":"ledger-key-2"`, `"configured_key_id":"stale-key-1"`, `"verifier":"cron"`} {
		if !strings.Contains(out, want) {
			t.Fatalf("log line missing %s: %s", want, out)
		}
	}
}

func TestBootCheckNotConfiguredWarnsAndStarts(t *testing.T) {
	var buf bytes.Buffer
	if err := BootCheck(testLogger(&buf), "orchestrator-core", "", ""); err != nil {
		t.Fatalf("unconfigured must not block start: %v", err)
	}
	if !strings.Contains(buf.String(), `"level":"WARN"`) {
		t.Fatalf("expected a warning: %s", buf.String())
	}
}

func TestBootCheckLogsIDAndFingerprintNeverTheKey(t *testing.T) {
	var buf bytes.Buffer
	key := keyWithURLSpecificChars(t)
	encoded := base64.RawURLEncoding.EncodeToString(key)
	if err := BootCheck(testLogger(&buf), "orchestrator-core", "control-2026-10", encoded); err != nil {
		t.Fatalf("valid key refused: %v", err)
	}
	out := buf.String()
	for _, want := range []string{`"decision_key_id":"control-2026-10"`, `"decision_key_fingerprint":"` + Fingerprint(key) + `"`} {
		if !strings.Contains(out, want) {
			t.Fatalf("missing %s in %s", want, out)
		}
	}
	if strings.Contains(out, encoded) || strings.Contains(out, base64.StdEncoding.EncodeToString(key)) {
		t.Fatalf("the key leaked into the log: %s", out)
	}
	if strings.Contains(out, "WARN") {
		t.Fatalf("a canonical key must not warn: %s", out)
	}
}

func TestBootCheckWarnsOnNonCanonicalButAccepts(t *testing.T) {
	var buf bytes.Buffer
	key := keyWithURLSpecificChars(t)
	if err := BootCheck(testLogger(&buf), "orchestrator-core", "k", base64.StdEncoding.EncodeToString(key)); err != nil {
		t.Fatalf("standard base64 should still be accepted: %v", err)
	}
	if !strings.Contains(buf.String(), `"level":"WARN"`) {
		t.Fatalf("expected a non-canonical warning: %s", buf.String())
	}
}

func TestBootCheckRefusesUnusableConfiguration(t *testing.T) {
	good := base64.RawURLEncoding.EncodeToString(testKey(3))
	for name, tc := range map[string]struct{ id, key string }{
		"garbage key":    {"k", "%%%garbage%%%"},
		"short key":      {"k", base64.RawURLEncoding.EncodeToString([]byte("tiny"))},
		"id only":        {"k", ""},
		"key only":       {"", good},
		"whitespace id":  {"   ", good},
		"whitespace key": {"k", "   "},
	} {
		var buf bytes.Buffer
		err := BootCheck(testLogger(&buf), "orchestrator-core", tc.id, tc.key)
		if err == nil {
			t.Fatalf("%s: expected start-up to be refused", name)
		}
		if tc.key != "" && strings.TrimSpace(tc.key) != "" && strings.Contains(err.Error(), tc.key) {
			t.Fatalf("%s: error echoes the key: %v", name, err)
		}
	}
}
