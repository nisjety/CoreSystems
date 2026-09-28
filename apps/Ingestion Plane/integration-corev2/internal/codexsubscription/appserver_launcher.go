package codexsubscription

import (
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

// The npm package of the pinned Codex release (@openai/codex 0.153.4) installs
// `codex` as a Node launcher, bin/codex.js. It only resolves the vendored native
// binary for the host architecture, <platform package>/vendor/<target
// triple>/bin/codex, and spawns it with the caller's arguments and environment
// plus these variables:
//
//   - CODEX_MANAGED_PACKAGE_ROOT: the realpath of the @openai/codex package.
//   - CODEX_MANAGED_BY_NPM=1: the launcher's package-manager marker. It clears
//     the other CODEX_MANAGED_BY_* markers first. npm is the fallback when it
//     detects neither bun, pnpm nor Vite+, which is the image's case: a global
//     npm install started without npm's user agent.
//
// It adds no PATH entry. The native binary prepends its own codex-path
// directory itself, from the package layout it finds next to its canonical
// executable (install-context and arg0 in the pinned source), so a symlink to
// the binary keeps that layout. The broker spawns the native binary directly,
// saving one Node process per call, and adds the same variables itself.
const (
	codexManagedPackageRootEnv = "CODEX_MANAGED_PACKAGE_ROOT"
	codexManagedByNPMEnv       = "CODEX_MANAGED_BY_NPM"
	codexNPMPackageName        = "@openai/codex"
)

// codexManagedEnvKeys are the variables the launcher sets or clears.
var codexManagedEnvKeys = []string{
	codexManagedPackageRootEnv,
	codexManagedByNPMEnv,
	"CODEX_MANAGED_BY_BUN",
	"CODEX_MANAGED_BY_PNPM",
	"CODEX_MANAGED_BY_VITE_PLUS",
}

// Launcher kinds reported at startup. They carry no path.
const (
	AppServerLauncherNativeNPM = "native-npm"
	AppServerLauncherNode      = "node-launcher"
	AppServerLauncherOther     = "other"
)

// DescribeAppServerLauncher reports how CODEX_APP_SERVER_COMMAND is launched:
// native-npm when it resolves to the vendored native binary of an npm-installed
// Codex (the broker then adds the launcher's variables), node-launcher when it
// resolves to the npm launcher script itself (which adds them), and other for
// anything else, including a command that cannot be found.
func DescribeAppServerLauncher(command string) string {
	resolved, ok := resolveCodexCommand(command)
	switch {
	case !ok:
		return AppServerLauncherOther
	case strings.EqualFold(filepath.Ext(resolved), ".js"):
		return AppServerLauncherNode
	case codexNPMPackageRootForNative(resolved) != "":
		return AppServerLauncherNativeNPM
	default:
		return AppServerLauncherOther
	}
}

// DefaultAppServerCommand is the npm launcher every image with the Codex CLI
// installs on PATH.
const DefaultAppServerCommand = "codex"

// AppServerCommandOrDefault returns command, or DefaultAppServerCommand when
// command is an absolute path that does not exist, such as the compose default
// /usr/local/bin/codex-native in an image built before that link was added.
// replaced reports the substitution, so the caller can say so at startup.
func AppServerCommandOrDefault(command string) (resolved string, replaced bool) {
	command = strings.TrimSpace(command)
	if !filepath.IsAbs(command) {
		return command, false
	}
	if _, ok := resolveCodexCommand(command); ok {
		return command, false
	}
	return DefaultAppServerCommand, true
}

// npmLauncherPackageRoot is the CODEX_MANAGED_PACKAGE_ROOT the npm launcher
// would pass when command is the vendored native binary of an npm-installed
// Codex, and "" when the command is the launcher itself (it sets the variables
// on its own), cannot be resolved, or lies outside such a package.
func npmLauncherPackageRoot(command string) string {
	resolved, ok := resolveCodexCommand(command)
	if !ok || strings.EqualFold(filepath.Ext(resolved), ".js") {
		return ""
	}
	return codexNPMPackageRootForNative(resolved)
}

// resolveCodexCommand finds the file exec.Command would start, following
// symlinks the way the kernel does. A bare name is looked up in PATH; a path is
// used as given.
func resolveCodexCommand(command string) (string, bool) {
	command = strings.TrimSpace(command)
	if command == "" {
		return "", false
	}
	path := command
	if !strings.ContainsAny(command, `/\`) {
		found, err := exec.LookPath(command)
		if err != nil {
			return "", false
		}
		path = found
	}
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		return "", false
	}
	info, err := os.Stat(resolved)
	if err != nil || !info.Mode().IsRegular() {
		return "", false
	}
	return resolved, true
}

// codexNPMPackageRootForNative maps a resolved native binary,
// <platform package>/vendor/<target triple>/bin/codex, back to the
// @openai/codex package whose launcher resolves it. The launcher resolves the
// platform package from its own directory, which npm nests under
// <root>/node_modules or hoists next to <root>, and falls back to <root>/vendor.
// A candidate counts only when it holds bin/codex.js and a package.json named
// @openai/codex; the platform package is published under that name too, but has
// no launcher.
func codexNPMPackageRootForNative(native string) string {
	switch filepath.Base(native) {
	case "codex", "codex.exe":
	default:
		return ""
	}
	binDir := filepath.Dir(native)
	if filepath.Base(binDir) != "bin" {
		return ""
	}
	vendorDir := filepath.Dir(filepath.Dir(binDir))
	if filepath.Base(vendorDir) != "vendor" {
		return ""
	}
	platformPackage := filepath.Dir(vendorDir)
	for _, candidate := range []string{
		filepath.Dir(filepath.Dir(filepath.Dir(platformPackage))), // <root>/node_modules/@openai/<platform package>
		filepath.Join(filepath.Dir(platformPackage), "codex"),     // <node_modules>/@openai/<platform package>
		platformPackage, // <root>/vendor, the launcher's fallback
	} {
		if !isCodexNPMPackage(candidate) {
			continue
		}
		root, err := filepath.EvalSymlinks(candidate)
		if err != nil {
			continue
		}
		return root
	}
	return ""
}

// isCodexNPMPackage reports whether dir is the @openai/codex package with its
// Node launcher.
func isCodexNPMPackage(dir string) bool {
	launcher, err := os.Stat(filepath.Join(dir, "bin", "codex.js"))
	if err != nil || !launcher.Mode().IsRegular() {
		return false
	}
	data, err := os.ReadFile(filepath.Join(dir, "package.json"))
	if err != nil {
		return false
	}
	var manifest struct {
		Name string `json:"name"`
	}
	return json.Unmarshal(data, &manifest) == nil && manifest.Name == codexNPMPackageName
}
