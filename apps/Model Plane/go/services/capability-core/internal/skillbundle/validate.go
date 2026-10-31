package skillbundle

import (
	"archive/zip"
	"bytes"
	"io"
	"io/fs"
	"strings"
)

// RefusalError names why an upload was refused. Code is a closed, content-free word that
// the HTTP layer returns as the error; it never carries a path or a file body.
type RefusalError struct {
	Code string
}

func (e *RefusalError) Error() string {
	return "skill bundle refused: " + e.Code
}

func refuse(code string) error {
	return &RefusalError{Code: code}
}

// Parse validates an uploaded zip and returns the bundle it holds. Every check runs before
// the bundle is stored: the zip must parse, every entry must be a plain file at a safe
// relative path, the caps must hold, the executable bit may only sit under scripts/, and
// SKILL.md must be at the root with its name and description in front matter.
//
// Symlinks, devices and other non-regular entries are refused, not skipped, so a bundle can
// never point a reader at a file outside itself. Directory entries carry no content and are
// ignored.
func Parse(body []byte) (Bundle, error) {
	reader, err := zip.NewReader(bytes.NewReader(body), int64(len(body)))
	if err != nil {
		return Bundle{}, refuse("not_a_zip")
	}
	if len(reader.File) > MaxFiles {
		return Bundle{}, refuse("too_many_files")
	}
	var (
		files   []File
		total   int64
		seen    = make(map[string]bool, len(reader.File))
		skillAt = -1
	)
	for _, entry := range reader.File {
		mode := entry.Mode()
		if mode.IsDir() {
			continue
		}
		if mode&fs.ModeSymlink != 0 {
			return Bundle{}, refuse("symlink_refused")
		}
		if !mode.IsRegular() {
			return Bundle{}, refuse("not_a_regular_file")
		}
		if !validPath(entry.Name) {
			return Bundle{}, refuse("unsafe_path")
		}
		if seen[entry.Name] {
			return Bundle{}, refuse("duplicate_path")
		}
		seen[entry.Name] = true
		if entry.UncompressedSize64 > MaxFileBytes {
			return Bundle{}, refuse("file_too_large")
		}
		if int64(entry.UncompressedSize64) > MaxTotalBytes-total {
			return Bundle{}, refuse("bundle_too_large")
		}
		content, err := readEntry(entry)
		if err != nil {
			return Bundle{}, err
		}
		total += int64(len(content))
		if total > MaxTotalBytes {
			return Bundle{}, refuse("bundle_too_large")
		}
		executable := mode.Perm()&0o111 != 0
		if executable && !strings.HasPrefix(entry.Name, scriptsDir) {
			return Bundle{}, refuse("exec_bit_outside_scripts")
		}
		file := File{
			Path:       entry.Name,
			SHA256:     bodyDigest(content),
			Size:       int64(len(content)),
			Executable: executable,
			Binary:     isBinary(content),
			Body:       content,
		}
		files = append(files, file)
		if entry.Name == SkillFileName {
			skillAt = len(files) - 1
		}
	}
	if skillAt < 0 {
		return Bundle{}, refuse("skill_md_missing")
	}
	if skillFile := files[skillAt]; skillFile.Binary || !validFrontMatter(skillFile.Body) {
		return Bundle{}, refuse("skill_md_front_matter")
	}
	sortFiles(files)
	return Bundle{Files: files, TotalBytes: total, ManifestSHA256: manifestDigest(files)}, nil
}

// readEntry reads one file body, never more than MaxFileBytes. Reading to EOF also makes the
// zip reader check the entry's CRC, so a corrupt entry is refused rather than stored.
func readEntry(entry *zip.File) ([]byte, error) {
	rc, err := entry.Open()
	if err != nil {
		return nil, refuse("not_a_zip")
	}
	defer rc.Close()
	content, err := io.ReadAll(io.LimitReader(rc, MaxFileBytes+1))
	if err != nil {
		return nil, refuse("not_a_zip")
	}
	if len(content) > MaxFileBytes {
		return nil, refuse("file_too_large")
	}
	return content, nil
}

// validPath accepts only a clean, relative, forward-slash path. A path with a drive colon, a
// backslash, a leading slash, an empty, "." or ".." segment, or a control character is refused.
func validPath(name string) bool {
	if name == "" || len(name) > 512 || strings.HasPrefix(name, "/") || strings.ContainsAny(name, "\\:") {
		return false
	}
	for _, r := range name {
		if r < 0x20 || r == 0x7f {
			return false
		}
	}
	for _, segment := range strings.Split(name, "/") {
		if segment == "" || segment == "." || segment == ".." {
			return false
		}
	}
	return true
}

// validFrontMatter requires a YAML-style front matter block at the top of SKILL.md with a
// non-empty name and description. Keys are read at column zero only, so an indented line in
// a body cannot satisfy the check.
func validFrontMatter(body []byte) bool {
	lines := strings.Split(string(body), "\n")
	if len(lines) == 0 || strings.TrimRight(lines[0], "\r") != "---" {
		return false
	}
	fields := make(map[string]string, 2)
	closed := false
	for _, raw := range lines[1:] {
		line := strings.TrimRight(raw, "\r")
		if line == "---" {
			closed = true
			break
		}
		key, value, found := strings.Cut(line, ":")
		if !found || key == "" || key != strings.TrimSpace(key) {
			continue
		}
		fields[key] = strings.TrimSpace(value)
	}
	return closed && fields["name"] != "" && fields["description"] != ""
}
