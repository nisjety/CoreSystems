package codexsubscription

import (
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"slices"
	"strings"
	"testing"

	"github.com/rs/zerolog"
)

// nativeCodexName is the vendored binary's file name, as the npm launcher
// builds it for the platform.
func nativeCodexName() string {
	if runtime.GOOS == "windows" {
		return "codex.exe"
	}
	return "codex"
}

// writeCodexNPMPackage writes the @openai/codex package files the launcher
// ships: package.json and bin/codex.js.
func writeCodexNPMPackage(t *testing.T, root string) {
	t.Helper()
	writeTestFile(t, filepath.Join(root, "package.json"), `{"name":"@openai/codex","version":"0.153.4","bin":{"codex":"bin/codex.js"}}`, 0o644)
	writeTestFile(t, filepath.Join(root, "bin", "codex.js"), "#!/usr/bin/env node\n", 0o755)
}

// writeVendoredCodex writes a platform package (published under the same
// package name, without a launcher) holding vendor/<triple>/bin/codex, and
// returns the binary's path. A nil binary writes a placeholder.
func writeVendoredCodex(t *testing.T, platformPackage string, binary []byte) string {
	t.Helper()
	writeTestFile(t, filepath.Join(platformPackage, "package.json"), `{"name":"@openai/codex","version":"0.153.4-linux-x64"}`, 0o644)
	triple := filepath.Join(platformPackage, "vendor", "x86_64-unknown-linux-musl")
	writeTestFile(t, filepath.Join(triple, "codex-package.json"), `{"layoutVersion":1,"entrypoint":"bin/codex","pathDir":"codex-path"}`, 0o644)
	if binary == nil {
		binary = []byte("native codex")
	}
	native := filepath.Join(triple, "bin", nativeCodexName())
	writeTestFile(t, native, string(binary), 0o755)
	return native
}

func writeTestFile(t *testing.T, path, content string, mode os.FileMode) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(content), mode); err != nil {
		t.Fatal(err)
	}
}

func realPath(t *testing.T, path string) string {
	t.Helper()
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		t.Fatal(err)
	}
	return resolved
}

func TestNPMLauncherPackageRoot(t *testing.T) {
	// npm nests the platform package under the launcher's package (the
	// image's global install).
	nestedRoot := filepath.Join(t.TempDir(), "lib", "node_modules", "@openai", "codex")
	writeCodexNPMPackage(t, nestedRoot)
	nested := writeVendoredCodex(t, filepath.Join(nestedRoot, "node_modules", "@openai", "codex-linux-x64"), nil)

	// A local install hoists it next to the launcher's package.
	hoistedModules := filepath.Join(t.TempDir(), "node_modules")
	writeCodexNPMPackage(t, filepath.Join(hoistedModules, "@openai", "codex"))
	hoisted := writeVendoredCodex(t, filepath.Join(hoistedModules, "@openai", "codex-linux-x64"), nil)

	// The launcher's fallback: <root>/vendor.
	fallbackRoot := filepath.Join(t.TempDir(), "@openai", "codex")
	writeCodexNPMPackage(t, fallbackRoot)
	fallback := writeVendoredCodex(t, fallbackRoot, nil)

	// A vendored-looking layout without the launcher package around it.
	orphan := writeVendoredCodex(t, filepath.Join(t.TempDir(), "node_modules", "@openai", "codex-linux-x64"), nil)

	// A standalone binary outside any npm package.
	standalone := filepath.Join(t.TempDir(), "bin", nativeCodexName())
	writeTestFile(t, standalone, "native codex", 0o755)

	for _, item := range []struct {
		name, command, wantRoot, wantKind string
	}{
		{"nested", nested, realPath(t, nestedRoot), AppServerLauncherNativeNPM},
		{"hoisted", hoisted, realPath(t, filepath.Join(hoistedModules, "@openai", "codex")), AppServerLauncherNativeNPM},
		{"fallback vendor", fallback, realPath(t, fallbackRoot), AppServerLauncherNativeNPM},
		{"node launcher", filepath.Join(nestedRoot, "bin", "codex.js"), "", AppServerLauncherNode},
		{"orphan platform package", orphan, "", AppServerLauncherOther},
		{"standalone binary", standalone, "", AppServerLauncherOther},
		{"missing command", filepath.Join(t.TempDir(), "missing", "codex"), "", AppServerLauncherOther},
		{"empty command", "  ", "", AppServerLauncherOther},
	} {
		if got := npmLauncherPackageRoot(item.command); got != item.wantRoot {
			t.Errorf("%s: package root = %q, want %q", item.name, got, item.wantRoot)
		}
		if got := DescribeAppServerLauncher(item.command); got != item.wantKind {
			t.Errorf("%s: launcher = %q, want %q", item.name, got, item.wantKind)
		}
	}

	// A bare name is looked up in PATH, like exec.Command does.
	t.Setenv("PATH", filepath.Dir(nested))
	if got := npmLauncherPackageRoot("codex"); got != realPath(t, nestedRoot) {
		t.Fatalf("PATH lookup: package root = %q", got)
	}

	// The image's stable path is a symlink; the launcher's view is the
	// symlink's target.
	link := filepath.Join(t.TempDir(), "codex-native")
	if err := os.Symlink(nested, link); err != nil {
		t.Logf("symlinks unavailable here, skipping the symlink case: %v", err)
		return
	}
	if got := npmLauncherPackageRoot(link); got != realPath(t, nestedRoot) {
		t.Fatalf("symlink: package root = %q", got)
	}
	if got := DescribeAppServerLauncher(link); got != AppServerLauncherNativeNPM {
		t.Fatalf("symlink: launcher = %q", got)
	}
	launcherLink := filepath.Join(t.TempDir(), "codex")
	if err := os.Symlink(filepath.Join(nestedRoot, "bin", "codex.js"), launcherLink); err != nil {
		t.Fatal(err)
	}
	if got := npmLauncherPackageRoot(launcherLink); got != "" {
		t.Fatalf("the node launcher's symlink got root %q; the launcher sets it itself", got)
	}
}

func managedEntries(environment []string) []string {
	var managed []string
	for _, entry := range environment {
		if strings.HasPrefix(strings.ToUpper(entry), "CODEX_MANAGED_") {
			managed = append(managed, entry)
		}
	}
	return managed
}

// The compose default names /usr/local/bin/codex-native, which only a newer
// image links. In an older image the broker must fall back to the npm launcher
// rather than fail every spawn.
func TestAppServerCommandOrDefault(t *testing.T) {
	dir := t.TempDir()
	missing := filepath.Join(dir, "bin", "codex-native")
	if got, replaced := AppServerCommandOrDefault(missing); got != DefaultAppServerCommand || !replaced {
		t.Fatalf("missing absolute command = %q, %v; want %q, true", got, replaced, DefaultAppServerCommand)
	}
	dangling := filepath.Join(dir, "codex-dangling")
	if err := os.Symlink(filepath.Join(dir, "gone"), dangling); err == nil {
		if got, replaced := AppServerCommandOrDefault(dangling); got != DefaultAppServerCommand || !replaced {
			t.Fatalf("dangling link = %q, %v; want %q, true", got, replaced, DefaultAppServerCommand)
		}
	}
	present := filepath.Join(dir, "codex-native")
	writeTestFile(t, present, "#!/bin/sh\n", 0o755)
	if got, replaced := AppServerCommandOrDefault(present); got != present || replaced {
		t.Fatalf("existing command = %q, %v; want it unchanged", got, replaced)
	}
	// A name is looked up in PATH at spawn time and is never replaced.
	for command, want := range map[string]string{"codex": "codex", " codex ": "codex", "codex-that-is-not-installed": "codex-that-is-not-installed"} {
		if got, replaced := AppServerCommandOrDefault(command); got != want || replaced {
			t.Fatalf("AppServerCommandOrDefault(%q) = %q, %v; want %q, false", command, got, replaced, want)
		}
	}
}

func TestCodexEnvironmentAddsTheNPMLauncherVariables(t *testing.T) {
	t.Setenv("CODEX_MANAGED_BY_PNPM", "1")
	t.Setenv("CODEX_MANAGED_BY_BUN", "1")
	t.Setenv("CODEX_MANAGED_BY_VITE_PLUS", "1")
	t.Setenv("CODEX_MANAGED_BY_NPM", "stale")
	t.Setenv("CODEX_MANAGED_PACKAGE_ROOT", "/stale/root")
	t.Setenv("PATH", "/usr/local/bin:/usr/bin")

	// Native binary: exactly what the launcher would pass.
	environment := codexEnvironment("/conn/home", "/conn/home/.ephemeral/x", "/usr/local/lib/node_modules/@openai/codex")
	want := []string{"CODEX_MANAGED_PACKAGE_ROOT=/usr/local/lib/node_modules/@openai/codex", "CODEX_MANAGED_BY_NPM=1"}
	if got := managedEntries(environment); !reflect.DeepEqual(got, want) {
		t.Fatalf("managed variables = %q, want %q", got, want)
	}
	// The launcher adds no PATH entry: the native binary prepends its own
	// codex-path directory.
	if !slices.Contains(environment, "PATH=/usr/local/bin:/usr/bin") {
		t.Fatal("PATH changed")
	}
	if !slices.Contains(environment, "CODEX_HOME=/conn/home") || !slices.Contains(environment, "CODEX_SQLITE_HOME=/conn/home/.ephemeral/x") {
		t.Fatal("connection scoping lost")
	}

	// The launcher itself (or anything else): the inherited environment
	// passes through, and the launcher replaces the markers on its own.
	inherited := managedEntries(codexEnvironment("/conn/home", "", ""))
	slices.Sort(inherited)
	wantInherited := []string{"CODEX_MANAGED_BY_BUN=1", "CODEX_MANAGED_BY_NPM=stale", "CODEX_MANAGED_BY_PNPM=1", "CODEX_MANAGED_BY_VITE_PLUS=1", "CODEX_MANAGED_PACKAGE_ROOT=/stale/root"}
	if !reflect.DeepEqual(inherited, wantInherited) {
		t.Fatalf("inherited managed variables = %q, want %q", inherited, wantInherited)
	}
}

// copyTestBinary returns the running test binary's bytes, so a copy can act as
// the vendored native Codex binary inside a fake npm layout.
func copyTestBinary(t *testing.T) []byte {
	t.Helper()
	executable, err := os.Executable()
	if err != nil {
		t.Fatalf("locate test binary: %v", err)
	}
	file, err := os.Open(executable)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	data, err := io.ReadAll(file)
	if err != nil {
		t.Fatal(err)
	}
	return data
}

// TestAppServerSpawnsTheNativeBinaryLikeTheNPMLauncher drives an inference
// through the vendored binary of a fake npm install: the process sees the
// launcher's variables, and thread/start carries the effort and service tier
// turn/start uses.
func TestAppServerSpawnsTheNativeBinaryLikeTheNPMLauncher(t *testing.T) {
	root := filepath.Join(t.TempDir(), "lib", "node_modules", "@openai", "codex")
	writeCodexNPMPackage(t, root)
	native := writeVendoredCodex(t, filepath.Join(root, "node_modules", "@openai", "codex-linux-x64"), copyTestBinary(t))
	t.Setenv(fakeAppServerEnv, "1")
	t.Setenv(fakeReportEnv, "1")
	t.Setenv("CODEX_MANAGED_BY_PNPM", "1")
	home := t.TempDir()
	if err := os.WriteFile(filepath.Join(home, "auth.json"), []byte("{}"), 0o600); err != nil {
		t.Fatal(err)
	}
	request := InvokeRequest{RequestID: "req-1", Model: "gpt-5.6-terra", Messages: []ChatMessage{{Role: "user", Content: "hello"}}, ReasoningEffort: "high", ServiceTier: "priority"}

	var logs strings.Builder
	response, err := NewProcessRunner(native, WithLogger(zerolog.New(&logs))).Invoke(t.Context(), home, request)
	if err != nil {
		t.Fatalf("Invoke: %v", err)
	}
	var report fakeAppServerReport
	if err := json.Unmarshal([]byte(response.Content), &report); err != nil {
		t.Fatalf("decode report %q: %v", response.Content, err)
	}
	// Windows hands a child its environment block sorted, so compare sets.
	want := []string{"CODEX_MANAGED_BY_NPM=1", "CODEX_MANAGED_PACKAGE_ROOT=" + realPath(t, root)}
	slices.Sort(report.Managed)
	if !reflect.DeepEqual(report.Managed, want) {
		t.Fatalf("native process managed variables = %q, want %q", report.Managed, want)
	}
	wantThread := fakeThreadStart{Effort: "high", ServiceTier: "priority", TurnEffort: "high", TurnServiceTier: "priority"}
	if report.ThreadStart != wantThread {
		t.Fatalf("reuse properties = %+v, want %+v", report.ThreadStart, wantThread)
	}
	if strings.Contains(logs.String(), "thread_effort_differs") {
		t.Fatalf("a matching thread effort was reported as different: %s", logs.String())
	}

	// A command that is not the vendored binary (here the plain test binary,
	// standing in for the npm launcher) gets the inherited environment only.
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	response, err = NewProcessRunner(executable).Invoke(t.Context(), home, request)
	if err != nil {
		t.Fatalf("Invoke: %v", err)
	}
	report = fakeAppServerReport{}
	if err := json.Unmarshal([]byte(response.Content), &report); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(report.Managed, []string{"CODEX_MANAGED_BY_PNPM=1"}) {
		t.Fatalf("launcher-path managed variables = %q", report.Managed)
	}
}
