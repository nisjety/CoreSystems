package telemetry

import (
	"io/fs"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"testing"
)

// allowedMetricAttributes is the closed set of attribute keys sandbox-manager may attach to a
// metric (WS06-S3). Both carry a closed value set: `method` is a fixed gRPC method name and
// `outcome` is a fixed decision enum. A thread, lease, run or workspace identifier as an
// attribute would put that identifier into the metrics store of every scraper.
var allowedMetricAttributes = map[string]bool{
	"method":  true,
	"outcome": true,
}

// attributeKey matches `attribute.String("key", ...)` and the other typed constructors, plus the
// `attribute.Key("key")` form.
var attributeKey = regexp.MustCompile(`attribute\.(?:String|Int|Int64|Bool|Float64|StringSlice|Int64Slice|BoolSlice|Float64Slice|IntSlice|Key)\(\s*"([^"]+)"`)

// attributeKeysIn returns every attribute key literal in source, comment lines excluded.
func attributeKeysIn(source string) []string {
	var keys []string
	for _, line := range strings.Split(source, "\n") {
		if strings.HasPrefix(strings.TrimSpace(line), "//") {
			continue
		}
		for _, m := range attributeKey.FindAllStringSubmatch(line, -1) {
			keys = append(keys, m[1])
		}
	}
	return keys
}

func TestAttributeKeysInFindsTypedConstructorsAndSkipsComments(t *testing.T) {
	src := `
	// attribute.String("thread_id", id)
	a := attribute.String("method", "AcquireLease")
	b := attribute.Key("lease_id")
	c := attribute.Int64("outcome", 1)
`
	got := attributeKeysIn(src)
	want := []string{"method", "lease_id", "outcome"}
	if len(got) != len(want) {
		t.Fatalf("keys = %v, want %v", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("keys = %v, want %v", got, want)
		}
	}
}

func TestMetricAttributesAreLimitedToMethodAndOutcome(t *testing.T) {
	root := filepath.Join("..", "..") // the sandbox-manager module root
	seen := map[string]bool{}
	var offences []string
	err := filepath.WalkDir(root, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			if d.Name() == "vendor" || d.Name() == "node_modules" {
				return filepath.SkipDir
			}
			return nil
		}
		if !strings.HasSuffix(path, ".go") || strings.HasSuffix(path, "_test.go") {
			return nil
		}
		raw, readErr := os.ReadFile(path)
		if readErr != nil {
			return readErr
		}
		for _, key := range attributeKeysIn(string(raw)) {
			seen[key] = true
			if !allowedMetricAttributes[key] {
				offences = append(offences, path+": attribute key "+key)
			}
		}
		return nil
	})
	if err != nil {
		t.Fatalf("walk: %v", err)
	}
	if len(seen) == 0 {
		t.Fatal("the scan found no attribute keys; the walk root or the pattern is wrong")
	}
	sort.Strings(offences)
	if len(offences) > 0 {
		t.Fatalf("metric attributes outside {method, outcome}:\n%s", strings.Join(offences, "\n"))
	}
	for _, key := range []string{"method", "outcome"} {
		if !seen[key] {
			t.Errorf("expected the scan to see attribute key %q", key)
		}
	}
}
