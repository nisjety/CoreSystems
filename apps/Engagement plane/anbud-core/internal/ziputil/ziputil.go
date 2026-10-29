// Package ziputil reads a ZIP archive that somebody else made, safely. The archive is the author's
// word and nothing else: its names, its declared sizes and its structure can all lie, so every
// limit is enforced on what is actually read, and nothing is written to disk.
//
// The threats it answers: a zip bomb (a small archive that expands to gigabytes, alone or nested
// inside another), a path that tries to leave its folder, an entry that is encrypted, a link or a
// device, a directory with millions of entries, and a name that is not text. An archive that looks
// hostile is refused whole; a file that is merely unusable (too large, encrypted, badly named) is
// skipped, and the rest is read.
package ziputil

import (
	"archive/zip"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"path"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/triodelab/anbud/internal/textsafe"
	"golang.org/x/text/unicode/norm"
)

var (
	// ErrNotArchive means the bytes are not a ZIP archive this code can read.
	ErrNotArchive = errors.New("ziputil: not a readable zip archive")
	// ErrTooLarge means the archive, or everything in it, is over a limit.
	ErrTooLarge = errors.New("ziputil: the archive or its contents exceed a size limit")
	// ErrTooManyEntries means the archive holds more entries than allowed.
	ErrTooManyEntries = errors.New("ziputil: the archive holds too many entries")
	// ErrBomb means an entry expands far beyond what it declares or what is allowed. The archive is
	// refused whole: an author who lies about sizes is hostile.
	ErrBomb = errors.New("ziputil: an entry expands beyond its limit (zip bomb)")
)

// Limits bound what an archive may be and what it may expand to.
type Limits struct {
	MaxArchiveBytes int64 // the archive itself
	MaxEntries      int   // entries in one archive's directory
	MaxEntryBytes   int64 // uncompressed bytes of one file
	MaxTotalBytes   int64 // uncompressed bytes of every file, nested archives included
	// MaxRatio is the largest uncompressed:compressed ratio of a file big enough for the ratio to
	// matter (over ratioFloor bytes).
	MaxRatio int
	// MaxDepth is how many archives may be inside one another: 1 is the archive itself and nothing nested.
	MaxDepth int
	// MaxTotalEntries is how many entries of directories a walk may visit in all, nested archives included.
	// The entries of one directory are bounded by MaxEntries, but a nested archive is opened for every entry
	// that names it, and entries may share their data, so the count of one directory says little of the work
	// of a walk. Four times MaxEntries when zero.
	MaxTotalEntries int
}

const (
	// ratioFloor is the size below which a high ratio is not judged: a few hundred bytes of one
	// letter compress by a large factor and mean no harm.
	ratioFloor = 1 << 20
	// maxPathRunes is the longest entry path kept.
	maxPathRunes = 240
	// eocdLen is the length of the end-of-central-directory record.
	eocdLen = 22
	// centralHeaderLen is the fixed part of a file header in the central directory, and its signature.
	centralHeaderLen       = 46
	centralHeaderSignature = 0x02014b50
	// zip64LocatorLen is the length of the ZIP64 end-of-central-directory locator, and its signature.
	zip64LocatorLen       = 20
	zip64LocatorSignature = 0x07064b50
	// zip64Count is the entry count that says the real one is in a ZIP64 record. It is more than
	// any limit here allows.
	zip64Count = 0xFFFF
)

// Entry is one file of an archive. Data is the entry's own copy of the bytes.
type Entry struct {
	// Path is the sanitised path: relative, slash-separated, in Unicode normal form C. A file in a
	// nested archive is under the nested archive's own path ("vedlegg.zip/krav.pdf").
	Path   string
	Size   int64
	SHA256 string
	Data   []byte
}

// SkipReason says why a file was not delivered.
type SkipReason string

const (
	SkipBadName       SkipReason = "bad_name"       // a path that leaves its folder, or is empty or too long, nested ones included
	SkipSpecial       SkipReason = "special"        // a link, a device or another entry that is not a plain file
	SkipEncrypted     SkipReason = "encrypted"      // the entry is encrypted
	SkipTooLarge      SkipReason = "too_large"      // the entry declares more than MaxEntryBytes
	SkipDuplicateName SkipReason = "duplicate_name" // the same path again, ignoring case
	SkipTooDeep       SkipReason = "too_deep"       // an archive inside archives past MaxDepth
	SkipUnreadable    SkipReason = "unreadable"     // the entry could not be read
)

// Skipped is a file that was not delivered, and why. Path is the sanitised path when there is one.
type Skipped struct {
	Path   string
	Reason SkipReason
}

// Report says what Walk did. Ignored counts junk that is never reported by name: system files and
// the like. Directories are not counted. Skipped lists the files that were not delivered, up to
// maxSkipped of them, and SkippedMore counts the ones that did not fit in the list: nothing is lost
// without a number.
type Report struct {
	Files       int
	Bytes       int64
	Ignored     int
	Skipped     []Skipped
	SkippedMore int
}

// maxSkipped bounds the skips a report lists, so a hostile archive cannot make it large.
const maxSkipped = 200

type walker struct {
	lim    Limits
	fn     func(Entry) error
	report Report
	total  int64
	// entries is how many entries of directories the walk has visited, in every archive it has opened.
	entries int
	seen    map[string]bool // lower-case sanitised paths delivered so far
}

// totalEntries is how many entries of directories a walk may visit.
func (w *walker) totalEntries() int {
	if w.lim.MaxTotalEntries > 0 {
		return w.lim.MaxTotalEntries
	}
	return 4 * w.lim.MaxEntries
}

// Walk reads the archive and calls fn once for each regular file, in archive order, with the
// file's bytes. fn's error stops the walk and is returned as it is. A nested archive (a file whose
// name ends in .zip and that is a ZIP) is opened and its files delivered under its path, up to
// Limits.MaxDepth; DOCX and XLSX files are ZIPs too, and are delivered as the files they are.
func Walk(ctx context.Context, data []byte, lim Limits, fn func(Entry) error) (Report, error) {
	w := &walker{lim: lim, fn: fn, seen: map[string]bool{}}
	err := w.archive(ctx, data, "", 1)
	return w.report, err
}

func (w *walker) skip(path string, reason SkipReason) {
	if len(w.report.Skipped) < maxSkipped {
		w.report.Skipped = append(w.report.Skipped, Skipped{Path: path, Reason: reason})
		return
	}
	w.report.SkippedMore++
}

func (w *walker) archive(ctx context.Context, data []byte, prefix string, depth int) error {
	if int64(len(data)) > w.lim.MaxArchiveBytes {
		return ErrTooLarge
	}
	r, at, err := newReader(data, w.lim.MaxEntries)
	if err != nil {
		return err
	}
	// The entries of a nested archive are the walk's as much as those of the first: an archive can name the
	// same nested archive again and again, and each is opened and its directory read.
	if w.entries += len(r.File); w.entries > w.totalEntries() {
		return ErrTooManyEntries
	}

	for _, f := range r.File {
		if err := ctx.Err(); err != nil {
			return err
		}
		if err := w.entry(ctx, f, prefix, depth, at); err != nil {
			return err
		}
	}
	return nil
}

func (w *walker) entry(ctx context.Context, f *zip.File, prefix string, depth int, at *budgetReaderAt) error {
	if strings.HasSuffix(f.Name, "/") || f.FileInfo().IsDir() {
		return nil
	}
	clean, ok := sanitise(f.Name)
	if !ok {
		w.skip(sanitiseForReport(f.Name), SkipBadName)
		return nil
	}
	if isJunk(clean) {
		w.report.Ignored++
		return nil
	}
	full := prefix + clean
	if utf8.RuneCountInString(full) > maxPathRunes {
		// Each name is within the bound, but the path of a file in a nested archive is all of them.
		w.skip(sanitiseForReport(full), SkipBadName)
		return nil
	}
	if !f.Mode().IsRegular() {
		w.skip(full, SkipSpecial)
		return nil
	}
	if f.Flags&0x1 != 0 {
		w.skip(full, SkipEncrypted)
		return nil
	}
	if f.UncompressedSize64 > uint64(w.lim.MaxEntryBytes) {
		w.skip(full, SkipTooLarge)
		return nil
	}
	if overRatio(int64(f.UncompressedSize64), int64(f.CompressedSize64), w.lim.MaxRatio) {
		return ErrBomb
	}
	key := strings.ToLower(full)
	if w.seen[key] {
		w.skip(full, SkipDuplicateName)
		return nil
	}

	before := at.read
	body, err := w.read(f)
	if err != nil {
		if errors.Is(err, ErrBomb) || errors.Is(err, ErrTooLarge) {
			return err
		}
		w.skip(full, SkipUnreadable)
		return nil
	}
	// The ratio is judged again on what the entry really took of the archive: the size it declares for itself
	// is the author's word, and an entry that says it is a hundredth of what it is passes the check above.
	if overRatio(int64(len(body)), at.read-before, w.lim.MaxRatio) {
		return ErrBomb
	}
	w.seen[key] = true

	if strings.HasSuffix(strings.ToLower(clean), ".zip") && isZip(body) {
		if depth >= w.lim.MaxDepth {
			w.skip(full, SkipTooDeep)
			return nil
		}
		err := w.archive(ctx, body, full+"/", depth+1)
		if errors.Is(err, ErrNotArchive) {
			// It starts like an archive and is not one: a damaged file, which is no reason to refuse the
			// files around it. Nothing of it was delivered, because the directory is read first.
			w.skip(full, SkipUnreadable)
			return nil
		}
		return err
	}

	sum := sha256.Sum256(body)
	w.report.Files++
	w.report.Bytes += int64(len(body))
	return w.fn(Entry{Path: full, Size: int64(len(body)), SHA256: hex.EncodeToString(sum[:]), Data: body})
}

// overRatio reports whether a file of size bytes that took stored bytes of the archive is compressed past ratio to
// one. A file too small to be judged is not (a few hundred bytes of one letter compress by a large factor and mean
// no harm), and nothing is judged by stored bytes that are none.
func overRatio(size, stored int64, ratio int) bool {
	return size > ratioFloor && stored > 0 && size/stored > int64(ratio)
}

// read returns an entry's bytes, counting what is actually read, whatever the header says.
func (w *walker) read(f *zip.File) ([]byte, error) {
	rc, err := f.Open()
	if err != nil {
		return nil, fmt.Errorf("open: %w", err)
	}
	defer rc.Close()
	return w.readAll(rc)
}

// readAll reads an entry's stream up to one byte past the limit, and refuses a stream that has more.
// The standard library already refuses a stream longer than the size the directory declares, so this
// does not fire today; it is what holds if that ever changes, and it is tested on its own.
func (w *walker) readAll(rc io.Reader) ([]byte, error) {
	body, err := io.ReadAll(io.LimitReader(rc, w.lim.MaxEntryBytes+1))
	// What was read is spent whether or not the entry turns out to be usable: one that fails its
	// checksum has cost its time and its memory all the same, and many of them must not be free.
	w.total += int64(len(body))
	switch {
	case int64(len(body)) > w.lim.MaxEntryBytes:
		return nil, ErrBomb
	case w.total > w.lim.MaxTotalBytes:
		// One budget serves every level, nested archives included: an archive that spends it is
		// refused whole.
		return nil, ErrTooLarge
	case err != nil:
		return nil, fmt.Errorf("read: %w", err)
	}
	return body, nil
}

func isZip(b []byte) bool {
	return len(b) >= 4 && b[0] == 'P' && b[1] == 'K' && (b[2] == 3 || b[2] == 5) && (b[3] == 4 || b[3] == 6)
}

// directoryRecord is what the end-of-central-directory record says about the directory.
type directoryRecord struct {
	count  int    // the entries it says there are: 16 bits
	size   uint32 // the bytes of the directory
	offset uint32 // where the directory starts
	at     int    // where the record is, in the file
}

// endRecord finds the end-of-central-directory record exactly as the standard library does
// (TestTheEndRecordReadIsTheLastOneThatFits and TestEntryCountAgreesWithTheStandardLibrary check it):
// the last signature in the final kilobyte, else in the final 65 kilobytes, and none when the comment
// length of that signature does not fit.
func endRecord(data []byte) (directoryRecord, bool) {
	size := int64(len(data))
	for i, window := range []int64{1024, 65 * 1024} {
		n := window
		if n > size {
			n = size
		}
		block := data[size-n:]
		if p := findSignature(block); p >= 0 {
			return directoryRecord{
				count:  int(binary.LittleEndian.Uint16(block[p+10 : p+12])),
				size:   binary.LittleEndian.Uint32(block[p+12 : p+16]),
				offset: binary.LittleEndian.Uint32(block[p+16 : p+20]),
				at:     int(size - n + int64(p)),
			}, true
		}
		if i == 1 || n == size {
			return directoryRecord{}, false
		}
	}
	return directoryRecord{}, false
}

// zip64Marker reports the values that send the standard library looking for a ZIP64 record. They are
// compared as the library compares them: a size of 0xFFFF (which is 16 bits, though the field is 32), an
// offset of 0xFFFFFFFF, and, in the caller, a count of 0xFFFF.
func (r directoryRecord) zip64Marker() bool {
	return r.size == 0xFFFF || r.size == 0xFFFFFFFF || r.offset == 0xFFFFFFFF
}

// hasZip64Locator reports whether the 20 bytes before the end record are a ZIP64 locator. Without one the
// library does not look for a ZIP64 record, and a marker in the end record is only a number.
func hasZip64Locator(data []byte, at int) bool {
	return at >= zip64LocatorLen && binary.LittleEndian.Uint32(data[at-zip64LocatorLen:]) == zip64LocatorSignature
}

// maxCounted is where the count of a directory's file headers stops: a directory with more than this
// is refused by any limit that can be set.
const maxCounted = 1 << 20

// entryCount says how many entries a ZIP file's directory holds, before the standard library
// allocates for any of them. The record's own count is 16 bits and is not what the library goes by: it
// reads file headers until one is bad, and compares only the low 16 bits with the record's count, so a
// directory of 65539 entries under a record that says 3 is read whole. The headers are counted here, from
// where the record says the directory starts and from where the library would look when something is
// written before the archive, and the larger count is the answer. A record that says ZIP64 (0xFFFF
// entries, or 0xFFFFFFFF in the size or the offset) holds more than any limit here allows, and is
// answered as 0xFFFF for the caller to refuse.
func entryCount(data []byte) (int, bool) {
	rec, ok := endRecord(data)
	if !ok {
		return 0, false
	}
	if rec.count == zip64Count || (rec.zip64Marker() && hasZip64Locator(data, rec.at)) {
		return zip64Count, true
	}
	count := rec.count
	for _, start := range []int64{int64(rec.offset), int64(rec.at) - int64(rec.size)} {
		if n := countHeaders(data, start, int64(rec.size), maxCounted); n > count {
			count = n
		}
	}
	return count, true
}

// countHeaders counts the file headers that follow one another from start, up to limit, within size
// bytes of the directory. It allocates nothing and reads only what the headers' own lengths say.
func countHeaders(data []byte, start, size int64, limit int) int {
	end := min(start+size, int64(len(data)))
	n := 0
	for p := start; start >= 0 && p+centralHeaderLen <= end && n < limit; n++ {
		if binary.LittleEndian.Uint32(data[p:]) != centralHeaderSignature {
			break
		}
		name := int64(binary.LittleEndian.Uint16(data[p+28:]))
		extra := int64(binary.LittleEndian.Uint16(data[p+30:]))
		comment := int64(binary.LittleEndian.Uint16(data[p+32:]))
		p += centralHeaderLen + name + extra + comment
	}
	return n
}

// EntryCount returns the number of entries a ZIP file's directory holds, before anything allocates for
// them, and false when the bytes have no directory. It is a cheap first look, and not the whole defence:
// see NewReader.
func EntryCount(data []byte) (int, bool) { return entryCount(data) }

// errDirectoryTooLarge is what the reader of a directory is told when it has read more than a directory of
// that many entries can be.
var errDirectoryTooLarge = errors.New("ziputil: the directory is larger than a directory of that many entries")

// directoryBytesPerEntry is how much of the file a reader of the directory may use for each entry the
// caller allows: a header, a name of the longest length, and room for the extra fields real archives carry.
const directoryBytesPerEntry = 1024

// directoryBytesBase is what a reader may use for the end of the file, where the end record and a comment
// are, on top of the entries.
const directoryBytesBase = 128 << 10

// NewReader opens a ZIP file for reading the directory and the entries, refusing a file with more than
// maxEntries entries before the standard library has allocated for them. It is the one way in for every
// reader of a ZIP-based file (an archive, a DOCX, a workbook): the library allocates for every header it
// reads, and reads until one is bad, whatever the end record says. So the count is looked at first, which
// costs nothing and catches the honest cases; and then the library is given a file that fails once it has
// read more than a directory of maxEntries entries can be, which stops the rest, such as a record that lies
// about the size of the directory, however it lies.
//
// It returns ErrTooManyEntries for a directory that is too large and ErrNotArchive for anything that is
// not a ZIP file.
func NewReader(data []byte, maxEntries int) (*zip.Reader, error) {
	r, _, err := newReader(data, maxEntries)
	return r, err
}

// newReader is NewReader, and also returns the reader the file is read through, which counts what is taken
// of it: what an entry really takes of the archive is what its ratio is judged by.
func newReader(data []byte, maxEntries int) (*zip.Reader, *budgetReaderAt, error) {
	count, ok := entryCount(data)
	if !ok {
		return nil, nil, ErrNotArchive
	}
	if count > maxEntries || count == zip64Count {
		return nil, nil, ErrTooManyEntries
	}
	at := &budgetReaderAt{r: bytes.NewReader(data), left: directoryBytesBase + int64(maxEntries)*directoryBytesPerEntry}
	r, err := zip.NewReader(at, int64(len(data)))
	switch {
	case errors.Is(err, errDirectoryTooLarge):
		return nil, nil, ErrTooManyEntries
	case err != nil && !errors.Is(err, zip.ErrInsecurePath):
		return nil, nil, ErrNotArchive
	case len(r.File) > maxEntries:
		return nil, nil, ErrTooManyEntries
	}
	// What is read from here on is the entries, and is not the directory's business.
	at.left = 1 << 62
	return r, at, nil
}

// budgetReaderAt reads from a file until it has read left bytes, and then fails: the budget of reading a
// directory. It counts every byte it hands out.
type budgetReaderAt struct {
	r    io.ReaderAt
	left int64
	read int64 // bytes handed out so far
}

func (b *budgetReaderAt) ReadAt(p []byte, off int64) (int, error) {
	if int64(len(p)) > b.left {
		return 0, errDirectoryTooLarge
	}
	n, err := b.r.ReadAt(p, off)
	b.left -= int64(n)
	b.read += int64(n)
	return n, err
}

func findSignature(b []byte) int {
	for i := len(b) - eocdLen; i >= 0; i-- {
		if b[i] == 'P' && b[i+1] == 'K' && b[i+2] == 0x05 && b[i+3] == 0x06 {
			commentLen := int(b[i+eocdLen-2]) | int(b[i+eocdLen-1])<<8
			if commentLen+eocdLen+i > len(b) {
				// A truncated comment. The standard library gives up on the window here, and does
				// not look further back, so neither does this: the two must read the same record.
				return -1
			}
			return i
		}
	}
	return -1
}

// sanitise returns the entry name as a safe relative path, or false when there is none. It turns
// backslashes into slashes, drops control characters, brings the name to Unicode normal form C,
// removes empty and dot segments, and refuses a name with a ".." segment, an empty name and one
// that is too long.
func sanitise(name string) (string, bool) {
	if !utf8.ValidString(name) {
		name = strings.ToValidUTF8(name, "�")
	}
	name = strings.Map(func(r rune) rune {
		switch {
		case r == '\\':
			return '/'
		case unicode.IsControl(r) || textsafe.Invisible(r):
			return -1 // controls, the bidirectional controls that make a name read backwards, and what cannot be seen
		}
		return r
	}, name)
	name = norm.NFC.String(name)

	var segments []string
	for _, seg := range strings.Split(name, "/") {
		switch seg {
		case "", ".":
			continue
		case "..":
			return "", false
		}
		segments = append(segments, seg)
	}
	clean := path.Join(segments...)
	if clean == "" || utf8.RuneCountInString(clean) > maxPathRunes {
		return "", false
	}
	return clean, true
}

// sanitiseForReport is what is shown of a name that was refused: its visible part, in normal form C,
// clipped. The name is not made safe to use as a path, because it is never used as one.
func sanitiseForReport(name string) string {
	name = strings.ToValidUTF8(name, "�")
	name = strings.Map(func(r rune) rune {
		if unicode.IsControl(r) || textsafe.Invisible(r) {
			return -1
		}
		return r
	}, name)
	name = norm.NFC.String(name)
	if utf8.RuneCountInString(name) > maxPathRunes {
		return string([]rune(name)[:maxPathRunes])
	}
	return name
}

// isJunk reports the files that operating systems add to an archive and that no tender is in.
func isJunk(clean string) bool {
	lower := strings.ToLower(clean)
	if strings.HasPrefix(lower, "__macosx/") || strings.Contains(lower, "/__macosx/") {
		return true
	}
	switch path.Base(lower) {
	case ".ds_store", "thumbs.db", "desktop.ini":
		return true
	}
	return false
}
