// Package skillbundle holds an org's skill bundles (WS19-S6): the zip an admin uploads for a
// skill, its validation, and a digest-addressed store that serves the files back.
//
// A bundle is org configuration, not conversation content. It is keyed by the org and the
// skill id it was uploaded for, and it never reads or writes agent_skills.
package skillbundle

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"sort"
	"strings"
	"unicode/utf8"
)

// Caps on one bundle. They are enforced when the zip is read, before any file body is
// kept, and the same numbers are the CHECK constraints in migration 0021.
const (
	MaxFiles      = 256
	MaxTotalBytes = 8 << 20
	MaxFileBytes  = 1 << 20

	// SkillFileName is the file every bundle must carry at its root. Its front matter
	// names the skill and says when to use it.
	SkillFileName = "SKILL.md"

	// scriptsDir is the only directory whose files may carry the executable bit.
	scriptsDir = "scripts/"
)

// File is one file of a bundle. Body is set only while a bundle is being uploaded: a
// manifest read leaves it empty, because the bytes are fetched by digest.
type File struct {
	Path       string
	SHA256     string
	Size       int64
	Executable bool
	Binary     bool
	Body       []byte
}

// Bundle is a validated upload: its files sorted by path, the totals, and the digest of
// the manifest, which covers every path, digest, size and mode and so changes with any of
// them.
type Bundle struct {
	Files          []File
	TotalBytes     int64
	ManifestSHA256 string
}

// manifestDigest hashes the manifest in path order, one line per file. Paths cannot hold a
// tab or a newline (validation refuses control characters), so the lines cannot be forged
// by a crafted name.
func manifestDigest(files []File) string {
	hash := sha256.New()
	for _, file := range files {
		executable := "0"
		if file.Executable {
			executable = "1"
		}
		fmt.Fprintf(hash, "%s\t%s\t%d\t%s\n", file.Path, file.SHA256, file.Size, executable)
	}
	return hex.EncodeToString(hash.Sum(nil))
}

// bodyDigest is the sha256 that addresses a file body. The blob store keys on it.
func bodyDigest(body []byte) string {
	sum := sha256.Sum256(body)
	return hex.EncodeToString(sum[:])
}

// isBinary reports whether a body is not UTF-8 text. A reader serves a binary file as
// metadata only, so a model never receives bytes it cannot read as text.
func isBinary(body []byte) bool {
	if strings.IndexByte(string(body), 0) >= 0 {
		return true
	}
	return !utf8.Valid(body)
}

// sortFiles puts files in path order, the order the manifest digest and every read use.
func sortFiles(files []File) {
	sort.Slice(files, func(i, j int) bool { return files[i].Path < files[j].Path })
}
