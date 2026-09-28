package config

import (
	"bytes"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

// codexNativeImagePath is where the API image links the vendored native Codex
// binary; the compose default CODEX_APP_SERVER_COMMAND points at it.
const codexNativeImagePath = "/usr/local/bin/codex-native"

// integrationCoreDir is the integration-corev2 module root.
func integrationCoreDir(t *testing.T) string {
	t.Helper()
	_, sourceFile, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("resolve test source path")
	}
	return filepath.Join(filepath.Dir(sourceFile), "..", "..")
}

func TestAPIImageLinksTheNativeCodexBinary(t *testing.T) {
	dockerfile, err := os.ReadFile(filepath.Join(integrationCoreDir(t), "Dockerfile"))
	if err != nil {
		t.Fatalf("read Dockerfile: %v", err)
	}
	text := strings.ReplaceAll(string(dockerfile), "\r\n", "\n")
	start := strings.Index(text, "\nFROM runtime-base AS api\n")
	if start < 0 {
		t.Fatal("api stage not found")
	}
	stage := text[start:]
	// In order: install the pinned launcher, keep it working, resolve the
	// binary it spawns, fail on a missing binary, link it, and prove the link
	// runs the launcher's release.
	position := 0
	for _, step := range []string{
		"--mount=type=bind,source=deploy/codex/resolve-native.cjs,target=/tmp/resolve-native.cjs",
		`npm install --global --omit=dev "@openai/codex@${CODEX_VERSION}" && \`,
		`codex --version && \`,
		`codex_native="$(node /tmp/resolve-native.cjs "$(readlink -f "$(command -v codex)")")" && \`,
		`test -x "$codex_native" && \`,
		`ln -s "$codex_native" ` + codexNativeImagePath + ` && \`,
		`test "$(` + codexNativeImagePath + ` --version)" = "$(codex --version)"`,
		"\nUSER app\n",
	} {
		index := strings.Index(stage[position:], step)
		if index < 0 {
			t.Fatalf("api stage lacks, or misorders, %q", step)
		}
		position += index + len(step)
	}
	if _, err := os.Stat(filepath.Join(integrationCoreDir(t), "deploy", "codex", "resolve-native.cjs")); err != nil {
		t.Fatalf("the bind-mounted resolver is missing: %v", err)
	}
}

// TestResolveNativeCodexMirrorsTheNPMLauncher runs the image's resolver
// against fake npm installs. It needs node, which the image build has; hosts
// without it skip.
func TestResolveNativeCodexMirrorsTheNPMLauncher(t *testing.T) {
	node, err := exec.LookPath("node")
	if err != nil {
		t.Skip("node is not installed")
	}
	script := filepath.Join(integrationCoreDir(t), "deploy", "codex", "resolve-native.cjs")
	resolve := func(launcher, platform, arch string) (string, error) {
		code := `const r = require(process.argv[1]);
try { process.stdout.write(r.resolveNativeCodex(process.argv[2], process.argv[3], process.argv[4])); }
catch (error) { process.stderr.write(error.message); process.exit(1); }`
		var stdout, stderr bytes.Buffer
		cmd := exec.Command(node, "-e", code, script, launcher, platform, arch)
		cmd.Stdout, cmd.Stderr = &stdout, &stderr
		if err := cmd.Run(); err != nil {
			return "", &resolveError{err: err, stderr: stderr.String()}
		}
		return stdout.String(), nil
	}
	write := func(path, content string) {
		t.Helper()
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(content), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	vendored := func(platformPackage, triple string) string {
		write(filepath.Join(platformPackage, "package.json"), `{"name":"@openai/codex","version":"0.153.4-linux"}`)
		binary := filepath.Join(platformPackage, "vendor", triple, "bin", "codex")
		write(binary, "native")
		return binary
	}
	launcherPackage := func(root string) string {
		write(filepath.Join(root, "package.json"), `{"name":"@openai/codex","version":"0.153.4"}`)
		launcher := filepath.Join(root, "bin", "codex.js")
		write(launcher, "#!/usr/bin/env node\n")
		return launcher
	}
	sameFile := func(got, want string) bool {
		gotReal, errGot := filepath.EvalSymlinks(got)
		wantReal, errWant := filepath.EvalSymlinks(want)
		return errGot == nil && errWant == nil && gotReal == wantReal
	}

	// The image's layout: npm nests the platform package.
	root := filepath.Join(t.TempDir(), "lib", "node_modules", "@openai", "codex")
	launcher := launcherPackage(root)
	nested := vendored(filepath.Join(root, "node_modules", "@openai", "codex-linux-x64"), "x86_64-unknown-linux-musl")
	if got, err := resolve(launcher, "linux", "x64"); err != nil || !sameFile(got, nested) {
		t.Fatalf("nested x64: got %q, %v; want %q", got, err, nested)
	}
	// arm64 selects its own package and triple, and fails when absent.
	if got, err := resolve(launcher, "linux", "arm64"); err == nil {
		t.Fatalf("arm64 without its package resolved %q", got)
	}
	arm := vendored(filepath.Join(root, "node_modules", "@openai", "codex-linux-arm64"), "aarch64-unknown-linux-musl")
	if got, err := resolve(launcher, "linux", "arm64"); err != nil || !sameFile(got, arm) {
		t.Fatalf("nested arm64: got %q, %v; want %q", got, err, arm)
	}
	for _, target := range [][2]string{{"linux", "ia32"}, {"darwin", "arm64"}, {"win32", "x64"}} {
		if got, err := resolve(launcher, target[0], target[1]); err == nil || !strings.Contains(err.Error(), "unsupported Codex platform") {
			t.Fatalf("%s/%s: got %q, %v; want unsupported platform", target[0], target[1], got, err)
		}
	}

	// A hoisted platform package, found from the launcher's directory.
	modules := filepath.Join(t.TempDir(), "node_modules")
	hoistedLauncher := launcherPackage(filepath.Join(modules, "@openai", "codex"))
	hoisted := vendored(filepath.Join(modules, "@openai", "codex-linux-x64"), "x86_64-unknown-linux-musl")
	if got, err := resolve(hoistedLauncher, "linux", "x64"); err != nil || !sameFile(got, hoisted) {
		t.Fatalf("hoisted: got %q, %v; want %q", got, err, hoisted)
	}

	// No platform package: the launcher's <package>/vendor fallback.
	fallbackRoot := filepath.Join(t.TempDir(), "codex")
	fallbackLauncher := launcherPackage(fallbackRoot)
	fallback := filepath.Join(fallbackRoot, "vendor", "x86_64-unknown-linux-musl", "bin", "codex")
	write(fallback, "native")
	if got, err := resolve(fallbackLauncher, "linux", "x64"); err != nil || !sameFile(got, fallback) {
		t.Fatalf("vendor fallback: got %q, %v; want %q", got, err, fallback)
	}

	// Nothing vendored at all: the build must fail.
	if got, err := resolve(launcherPackage(filepath.Join(t.TempDir(), "codex")), "linux", "x64"); err == nil || !strings.Contains(err.Error(), "missing vendored Codex binary") {
		t.Fatalf("empty install: got %q, %v; want a missing-binary failure", got, err)
	}
}

type resolveError struct {
	err    error
	stderr string
}

func (e *resolveError) Error() string { return e.err.Error() + ": " + e.stderr }
