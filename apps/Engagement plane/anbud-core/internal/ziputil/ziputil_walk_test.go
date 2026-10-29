package ziputil

import (
	"bytes"
	"compress/flate"
	"encoding/binary"
	"errors"
	"fmt"
	"hash/crc32"
	"math/rand"
	"testing"
)

// What the tests of the standard library's writer cannot make: an archive whose directory says what it likes. The
// writer here puts a local header and the data of each entry in the file, and then a directory in which the
// entries may say any size, and may share the data of another.

// rawEntry is an entry of rawZip.
type rawEntry struct {
	name       string
	method     uint16 // 0 stored, 8 deflated
	crc        uint32
	compressed uint32 // what the directory says the stored bytes come to
	size       uint32 // what the directory says the file comes to
	data       []byte // what is stored: the file as it is, or deflated
	shared     bool   // the entry has no data of its own: the directory points at the data of the one before
}

// stored is an entry of a file kept as it is.
func stored(name string, data []byte) rawEntry {
	return rawEntry{name: name, method: 0, crc: crc32.ChecksumIEEE(data), compressed: uint32(len(data)), size: uint32(len(data)), data: data}
}

// deflated is an entry of a file that is deflated, which says that the stored bytes come to declared.
func deflated(t *testing.T, name string, data []byte, declared uint32) rawEntry {
	t.Helper()
	var packed bytes.Buffer
	fw, err := flate.NewWriter(&packed, flate.BestSpeed)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := fw.Write(data); err != nil {
		t.Fatal(err)
	}
	if err := fw.Close(); err != nil {
		t.Fatal(err)
	}
	return rawEntry{name: name, method: 8, crc: crc32.ChecksumIEEE(data), compressed: declared, size: uint32(len(data)), data: packed.Bytes()}
}

// rawZip writes the entries as an archive, with no flags and no data descriptors: the sizes the standard library
// goes by are those of the directory.
func rawZip(entries []rawEntry) []byte {
	le := binary.LittleEndian
	var out bytes.Buffer
	offsets := make([]uint32, len(entries))
	for i, e := range entries {
		if e.shared && i > 0 {
			offsets[i] = offsets[i-1]
			continue
		}
		offsets[i] = uint32(out.Len())
		local := make([]byte, 30)
		le.PutUint32(local[0:], 0x04034b50)
		le.PutUint16(local[4:], 20)
		le.PutUint16(local[8:], e.method)
		le.PutUint32(local[14:], e.crc)
		le.PutUint32(local[18:], e.compressed)
		le.PutUint32(local[22:], e.size)
		le.PutUint16(local[26:], uint16(len(e.name)))
		out.Write(local)
		out.WriteString(e.name)
		out.Write(e.data)
	}
	dirStart := out.Len()
	for i, e := range entries {
		cd := make([]byte, 46)
		le.PutUint32(cd[0:], 0x02014b50)
		le.PutUint16(cd[4:], 20)
		le.PutUint16(cd[6:], 20)
		le.PutUint16(cd[10:], e.method)
		le.PutUint32(cd[16:], e.crc)
		le.PutUint32(cd[20:], e.compressed)
		le.PutUint32(cd[24:], e.size)
		le.PutUint16(cd[28:], uint16(len(e.name)))
		le.PutUint32(cd[42:], offsets[i])
		out.Write(cd)
		out.WriteString(e.name)
	}
	rec := make([]byte, eocdLen)
	le.PutUint32(rec[0:], 0x06054b50)
	le.PutUint16(rec[8:], uint16(len(entries)))
	le.PutUint16(rec[10:], uint16(len(entries)))
	le.PutUint32(rec[12:], uint32(out.Len()-dirStart))
	le.PutUint32(rec[16:], uint32(dirStart))
	out.Write(rec)
	return out.Bytes()
}

// sharing is an archive of n entries named by name, all of which are the one file: the directory has a
// record for each, and every record points at the same data.
func sharing(n int, body []byte, name func(int) string) []byte {
	entries := make([]rawEntry, n)
	for i := range entries {
		entries[i] = stored(name(i), body)
		entries[i].shared = i > 0
	}
	return rawZip(entries)
}

// An archive can name the same nested archive again and again, and each is opened and its directory read: the
// directories of fifty entries, three deep, are two thousand five hundred nested archives, and the budget of bytes
// does not notice, because each is a few bytes. The entries of every directory are counted across the walk.
func TestNestedArchivesThatShareTheirDataAreCountedAcrossTheWalk(t *testing.T) {
	t.Parallel()

	leaf := build(t) // an empty archive: its end record and nothing else
	if !isZip(leaf) {
		t.Fatal("the test needs a leaf that is taken for an archive")
	}
	middle := sharing(50, leaf, func(i int) string { return fmt.Sprintf("b%02d.zip", i) })
	top := sharing(50, middle, func(i int) string { return fmt.Sprintf("a%02d.zip", i) })
	if len(top) > 1<<20 {
		t.Fatalf("the archive is %d bytes", len(top))
	}
	lim := limits() // 50 entries in a directory, and so 200 in a walk
	if _, err := Walk(t.Context(), top, lim, func(Entry) error { return nil }); !errors.Is(err, ErrTooManyEntries) {
		t.Errorf("err = %v, want ErrTooManyEntries: the walk opened %d nested archives", err, 50*50)
	}
}

// The limit across the walk is the limit of one directory times four unless it is set, and it is exact: a walk
// that visits exactly as many entries as it may is read, and one more is refused. A set limit is the limit.
func TestTheLimitOnEntriesAcrossTheWalkIsExact(t *testing.T) {
	t.Parallel()

	inner := func(prefix string) string {
		var files []file
		for i := range 10 {
			files = append(files, file{name: fmt.Sprintf("%s%d.txt", prefix, i), data: fmt.Sprintf("%s %d", prefix, i)})
		}
		return string(build(t, files...))
	}
	// A directory of ten entries, k of them archives of ten files: 10 + 10k entries are visited.
	archive := func(k int) []byte {
		var files []file
		for i := range k {
			files = append(files, file{name: fmt.Sprintf("n%d.zip", i), data: inner(fmt.Sprintf("n%d-", i))})
		}
		for i := k; i < 10; i++ {
			files = append(files, file{name: fmt.Sprintf("f%d.txt", i), data: "x"})
		}
		return build(t, files...)
	}
	lim := limits()
	lim.MaxEntries = 10 // four times ten is forty entries in a walk
	if got, _, err := collect(t, archive(3), lim); err != nil || len(got) != 7+30 {
		t.Errorf("forty entries visited: %d files, %v", len(got), err)
	}
	if _, _, err := collect(t, archive(4), lim); !errors.Is(err, ErrTooManyEntries) {
		t.Errorf("fifty entries visited: err = %v, want ErrTooManyEntries", err)
	}
	lim.MaxTotalEntries = 50
	if got, _, err := collect(t, archive(4), lim); err != nil || len(got) != 6+40 {
		t.Errorf("fifty entries visited, with fifty allowed: %d files, %v", len(got), err)
	}
	lim.MaxTotalEntries = 49
	if _, _, err := collect(t, archive(4), lim); !errors.Is(err, ErrTooManyEntries) {
		t.Errorf("fifty entries visited, with forty-nine allowed: err = %v, want ErrTooManyEntries", err)
	}
}

// The ratio of an entry is judged by what it took of the archive and not only by what it says it is: an entry
// that declares itself a fiftieth of its size passes the first check, and is not a fiftieth.
func TestAnEntryThatSaysItIsSmallerThanItIsIsJudgedByWhatItTook(t *testing.T) {
	t.Parallel()

	zeros := make([]byte, 8<<20) // deflates to some eight kilobytes
	honest := deflated(t, "nuller.bin", zeros, 0)
	honest.compressed = uint32(len(honest.data))
	lim := limits()
	lim.MaxArchiveBytes = 1 << 20
	lim.MaxEntryBytes = 16 << 20
	lim.MaxTotalBytes = 64 << 20
	lim.MaxRatio = 100

	if _, _, err := collect(t, rawZip([]rawEntry{honest}), lim); !errors.Is(err, ErrBomb) {
		t.Fatalf("an entry that says what it is: err = %v, want ErrBomb", err)
	}
	lying := deflated(t, "nuller.bin", zeros, uint32(len(zeros)/50))
	if got, _, err := collect(t, rawZip([]rawEntry{lying}), lim); !errors.Is(err, ErrBomb) {
		t.Errorf("an entry that says it is a fiftieth of its size: %d files, err = %v, want ErrBomb", len(got), err)
	}
	// Text that compresses as text does is not a bomb.
	rng := rand.New(rand.NewSource(7))
	var prose bytes.Buffer
	for prose.Len() < 2<<20 {
		fmt.Fprintf(&prose, "ord%d ", rng.Intn(300)) // some three hundred words, in no order
	}
	text := prose.Bytes()
	ok := deflated(t, "krav.txt", text, 0)
	ok.compressed = uint32(len(ok.data))
	if ratio := len(text) / len(ok.data); ratio > 100 || ratio < 2 {
		t.Fatalf("the text compresses %d:1: the test needs a ratio under the limit and over one", ratio)
	}
	if got, _, err := collect(t, rawZip([]rawEntry{ok}), lim); err != nil || len(got) != 1 {
		t.Errorf("text that compresses well: %d files, %v", len(got), err)
	}
}

func TestTheRatioIsJudgedAtTheEdgesOfItsRules(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct {
		size, stored int64
		ratio        int
		want         bool
	}{
		"a file of the floor is not judged":             {ratioFloor, 1, 100, false},
		"a file one over the floor is":                  {ratioFloor + 1, 1, 100, true},
		"a ratio of the limit":                          {2_000_000, 20_000, 100, false},
		"a ratio of the limit and a part":               {2_000_999, 20_000, 100, false},
		"a ratio one over the limit":                    {2_020_000, 20_000, 100, true},
		"nothing stored is not judged":                  {10 << 20, 0, 100, false},
		"a negative amount stored is not judged":        {10 << 20, -5, 100, false},
		"a file that is stored as it is":                {10 << 20, 10 << 20, 100, false},
		"a limit of one, and a ratio of two":            {10 << 20, 5 << 20, 1, true},
		"a limit of one, and a ratio of one and a part": {10 << 20, 10<<20 - 1, 1, false},
		"a ratio of exactly the limit of one":           {10 << 20, 10 << 20, 1, false},
	} {
		if got := overRatio(tc.size, tc.stored, tc.ratio); got != tc.want {
			t.Errorf("%s: overRatio(%d, %d, %d) = %v, want %v", name, tc.size, tc.stored, tc.ratio, got, tc.want)
		}
	}
}

// What an entry took is counted from where it began, and not from the start of the archive: a lying entry that comes
// after bulky ones is judged by its own bytes and not by all the bytes before it.
func TestWhatAnEntryTookIsCountedFromWhereItBegan(t *testing.T) {
	t.Parallel()

	bulky := make([]byte, 3<<20) // incompressible, and kept as it is
	_, _ = rand.New(rand.NewSource(11)).Read(bulky)
	zeros := make([]byte, 8<<20)
	lying := deflated(t, "nuller.bin", zeros, uint32(len(zeros)/50))
	lim := limits()
	lim.MaxArchiveBytes = 8 << 20
	lim.MaxEntryBytes = 16 << 20
	lim.MaxTotalBytes = 64 << 20
	lim.MaxRatio = 100
	data := rawZip([]rawEntry{stored("tykk.bin", bulky), lying})
	if got, _, err := collect(t, data, lim); !errors.Is(err, ErrBomb) {
		t.Errorf("an entry that lies, after a bulky one: %d files, err = %v, want ErrBomb", len(got), err)
	}
}

// An entry that declares a ratio of a bomb is refused before it is read: the archive is refused whole for it, and
// not only skipped for being unreadable, which is what its bytes would make it if they were read (they are not a
// deflated file at all).
func TestAnEntryThatDeclaresItselfABombIsRefusedBeforeItIsRead(t *testing.T) {
	t.Parallel()

	garbage := bytes.Repeat([]byte{0xFF}, 4096) // not a deflate stream: reading it fails
	bomb := rawEntry{name: "bombe.bin", method: 8, crc: 1, compressed: uint32(len(garbage)), size: 8 << 20, data: garbage}
	lim := limits()
	lim.MaxEntryBytes = 16 << 20
	lim.MaxTotalBytes = 64 << 20
	lim.MaxRatio = 100
	if _, _, err := collect(t, rawZip([]rawEntry{bomb}), lim); !errors.Is(err, ErrBomb) {
		t.Errorf("err = %v, want ErrBomb: the declared ratio of 2048 was not judged before the read", err)
	}
	// With a ratio that is not a bomb's the same bytes are only unreadable: the entry is skipped and the rest is read.
	fine := bomb
	fine.size = 1 << 20
	got, report, err := collect(t, rawZip([]rawEntry{fine, stored("god.txt", []byte("innhold"))}), lim)
	if err != nil || got["god.txt"] != "innhold" || len(report.Skipped) != 1 || report.Skipped[0].Reason != SkipUnreadable {
		t.Errorf("an entry that is only unreadable: %v, delivered %v, skipped %+v", err, keys(got), report.Skipped)
	}
}
