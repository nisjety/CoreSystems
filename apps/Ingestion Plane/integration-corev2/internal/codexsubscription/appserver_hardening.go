package codexsubscription

import (
	"bytes"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/pelletier/go-toml/v2"
)

const (
	codexConfigFileName  = "config.toml"
	maxCodexConfigBytes  = 1 << 20
	mcpServersConfigKey  = "mcp_servers"
	codexSQLiteHomeKey   = "sqlite_home"
	codexAppServerSubcmd = "app-server"
)

// errUnrecognizedMCPConfig refuses an inference process whose connection
// config.toml declares MCP servers in a form the broker cannot disable by
// name. It carries no config content.
var errUnrecognizedMCPConfig = errors.New("codex subscription connection config.toml declares MCP servers the broker cannot disable")

// mcpServerName is a server name a dotted -c path can address as a bare key.
var mcpServerName = regexp.MustCompile(`^[A-Za-z0-9_-]{1,64}$`)

// configuredMCPServers lists the MCP servers the connection home's
// config.toml declares, so each can be disabled by name. A CLI override
// merges into config.toml table by table (Codex config/src/merge.rs), so
// `-c mcp_servers={}` would leave every declared server in place.
//
// The file is decoded as TOML, so a string or a comment can never hide a
// declaration from the scan, and an escaped key resolves to the name Codex
// sees. It fails closed with errUnrecognizedMCPConfig on anything it cannot
// disable by name: a file that does not decode, an mcp_servers value that is
// not a table, a server that is not a table or whose name is outside the
// bare-key set, and any profile-scoped mcp_servers. A missing file, or one
// that cannot spell mcp_servers even through an escape, declares nothing and
// is not decoded.
func configuredMCPServers(codeHome string) ([]string, error) {
	file, err := os.Open(filepath.Join(codeHome, codexConfigFileName))
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("inspect Codex connection config: %w", err)
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, maxCodexConfigBytes+1))
	if err != nil {
		return nil, fmt.Errorf("read Codex connection config: %w", err)
	}
	if len(data) > maxCodexConfigBytes {
		return nil, errUnrecognizedMCPConfig
	}
	// A key can spell mcp_servers only literally or through a \u, \U or (in
	// TOML 1.1) \x escape. \x is not TOML 1.0, so such a file fails to
	// decode below and is refused.
	if !bytes.Contains(data, []byte(mcpServersConfigKey)) && !bytes.Contains(data, []byte(`\u`)) &&
		!bytes.Contains(data, []byte(`\U`)) && !bytes.Contains(data, []byte(`\x`)) {
		return nil, nil
	}
	var document map[string]any
	if err := toml.Unmarshal(data, &document); err != nil {
		return nil, errUnrecognizedMCPConfig
	}
	if profiles, ok := document["profiles"].(map[string]any); ok {
		for _, profile := range profiles {
			if table, ok := profile.(map[string]any); ok {
				if _, scoped := table[mcpServersConfigKey]; scoped {
					return nil, errUnrecognizedMCPConfig
				}
			}
		}
	}
	declared, ok := document[mcpServersConfigKey]
	if !ok {
		return nil, nil
	}
	servers, ok := declared.(map[string]any)
	if !ok {
		return nil, errUnrecognizedMCPConfig
	}
	var names []string
	for name, server := range servers {
		if _, isTable := server.(map[string]any); !isTable || !mcpServerName.MatchString(name) {
			return nil, errUnrecognizedMCPConfig
		}
		names = append(names, name)
	}
	slices.Sort(names)
	return names, nil
}

// codexProcessArgs is codexAppServerArgs plus the hardening that depends on
// the spawned process: sqlite_home pinned to its throwaway directory (a
// sqlite_home in config.toml would otherwise win over CODEX_SQLITE_HOME) and
// every MCP server config.toml declares turned off by name. The overrides go
// before the subcommand, with the other root-level -c flags. Unhardened, the
// list is exactly codexAppServerArgs(false).
func codexProcessArgs(hardened bool, sqliteHome string, mcpServers []string) ([]string, error) {
	base := codexAppServerArgs(hardened)
	if !hardened {
		return base, nil
	}
	quoted, err := tomlBasicString(sqliteHome)
	if err != nil {
		return nil, err
	}
	if base[len(base)-1] != codexAppServerSubcmd {
		return nil, errors.New("codex app-server argument list does not end with its subcommand")
	}
	args := make([]string, 0, len(base)+2+2*len(mcpServers))
	args = append(args, base[:len(base)-1]...)
	args = append(args, "-c", codexSQLiteHomeKey+"="+quoted)
	for _, name := range mcpServers {
		args = append(args, "-c", mcpServersConfigKey+"."+name+".enabled=false")
	}
	return append(args, codexAppServerSubcmd), nil
}

// tomlBasicString quotes value as a TOML basic string, the form a -c value
// is parsed in. Quote, backslash and control characters are escaped; a value
// that is not valid UTF-8 cannot be written faithfully and is refused.
func tomlBasicString(value string) (string, error) {
	if !utf8.ValidString(value) {
		return "", errors.New("codex override value is not valid UTF-8")
	}
	var quoted strings.Builder
	quoted.WriteByte('"')
	for _, char := range value {
		switch {
		case char == '"':
			quoted.WriteString(`\"`)
		case char == '\\':
			quoted.WriteString(`\\`)
		case char < 0x20 || char == 0x7f:
			fmt.Fprintf(&quoted, `\u%04X`, char)
		default:
			quoted.WriteRune(char)
		}
	}
	quoted.WriteByte('"')
	return quoted.String(), nil
}

// sweepEphemeralHomes removes every connection's throwaway Codex state
// directory below home and reports how many it removed. Only a spawned Codex
// process uses one, and no such process outlives the broker instance that
// spawned it (compose runs the broker under init: true, so the container's
// whole process tree ends with it). At startup every entry is therefore left
// over from a crash, an OOM kill or a redeploy, and may hold the TRACE-level
// prompt log that must not survive its process. Symbolic links are never
// followed.
func sweepEphemeralHomes(home string) int {
	connections, err := os.ReadDir(home)
	if err != nil {
		return 0
	}
	removed := 0
	for _, connection := range connections {
		if !connection.IsDir() {
			continue
		}
		parent := filepath.Join(home, connection.Name(), ephemeralDirName)
		info, err := os.Lstat(parent)
		if err != nil || !info.IsDir() {
			continue
		}
		children, err := os.ReadDir(parent)
		if err != nil {
			continue
		}
		for _, child := range children {
			if os.RemoveAll(filepath.Join(parent, child.Name())) == nil {
				removed++
			}
		}
	}
	return removed
}
