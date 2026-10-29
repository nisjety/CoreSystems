package docread

import (
	"archive/zip"
	"bytes"
	"compress/zlib"
	"fmt"
	"os"
	"runtime/debug"
	"strings"
	"testing"
	"time"
)

// childModeEnv selects what the test binary does when the tests run it as the PDF child process.
const childModeEnv = "ANBUD_TEST_PDF_CHILD"

// TestMain lets the test binary stand in for the service binary: ChildPDF runs it as a child
// process, and the environment says which part of the protocol to play.
func TestMain(m *testing.M) {
	switch os.Getenv(childModeEnv) {
	case "":
		os.Exit(m.Run())
	case "real":
		os.Exit(RunPDFChild(os.Args[1:], os.Stdin, os.Stdout))
	case "main":
		// Plays the service binary started with ChildArg: the limits it sets itself are the real ones.
		os.Exit(ChildMain())
	case "under":
		// Plays a reader that holds a good deal of memory, but less than the limit it runs under, and lives on.
		startMemoryWatchdog(64<<20, 5*time.Millisecond)
		held := make([]byte, 40<<20)
		for i := range held {
			held[i] = byte(i)
		}
		time.Sleep(300 * time.Millisecond) // the watchdog looks sixty times in this
		fmt.Printf("alive %d", len(held))
	case "limits":
		// Plays the child up to the point where it has limited its memory, and says what its soft limit is and, where
		// the system has one, how willing the kernel is to end it first.
		limitMemory(64 << 20)
		fmt.Println(debug.SetMemoryLimit(-1))
		if score, err := os.ReadFile("/proc/self/oom_score_adj"); err == nil {
			fmt.Print(strings.TrimSpace(string(score)))
		}
	case "sleep":
		time.Sleep(time.Hour)
	case "flood":
		chunk := strings.Repeat("x", 1<<16)
		for {
			if _, err := os.Stdout.WriteString(chunk); err != nil {
				os.Exit(0)
			}
		}
	case "crash":
		os.Exit(3)
	case "garbage":
		fmt.Print("this is not json")
	case "leak":
		// Plays a child that reports what it can see of its parent's environment.
		fmt.Printf(`{"pages":[%q],"total":1}`, os.Getenv("ANBUD_SECRET_PROBE"))
	case "error-code":
		fmt.Print(`{"error":"encrypted"}`)
	case "unknown-error":
		fmt.Print(`{"error":"something nobody planned for"}`)
	case "balloon":
		// Plays a reader that allocates for good, under the watchdog the real child runs with.
		startMemoryWatchdog(64<<20, 5*time.Millisecond)
		for {
			b := make([]byte, 4<<20)
			for i := range b {
				b[i] = byte(i)
			}
			ballooned = append(ballooned, b) // kept, so that it is never collected
		}
	case "cwd":
		// Plays a child that reports the directory it was started in.
		dir, _ := os.Getwd()
		fmt.Printf(`{"pages":[%q],"total":1}`, dir)
	}
}

// ballooned holds what the balloon mode allocates, for as long as the process lives.
var ballooned [][]byte

// miniPDF writes a PDF with one page for each string. A page whose text is empty has no text at all,
// like a scanned page that is only a picture. The text is in Helvetica, WinAnsi encoded, so æøå
// are written as their WinAnsi bytes.
func miniPDF(pages ...string) []byte {
	var b bytes.Buffer
	var offsets []int
	obj := func(body string) {
		offsets = append(offsets, b.Len())
		fmt.Fprintf(&b, "%d 0 obj\n%s\nendobj\n", len(offsets), body)
	}
	b.WriteString("%PDF-1.4\n")
	obj("<< /Type /Catalog /Pages 2 0 R >>")
	kids := make([]string, len(pages))
	for i := range pages {
		kids[i] = fmt.Sprintf("%d 0 R", 4+2*i)
	}
	obj(fmt.Sprintf("<< /Type /Pages /Kids [%s] /Count %d >>", strings.Join(kids, " "), len(pages)))
	obj("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>")
	for i, text := range pages {
		contentNo := 5 + 2*i
		obj(fmt.Sprintf("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R >> >> /Contents %d 0 R >>", contentNo))
		stream := ""
		if text != "" {
			stream = "BT /F1 12 Tf 50 780 Td (" + winAnsi(text) + ") Tj ET"
		}
		obj(fmt.Sprintf("<< /Length %d >>\nstream\n%s\nendstream", len(stream), stream))
	}
	xref := b.Len()
	fmt.Fprintf(&b, "xref\n0 %d\n0000000000 65535 f \n", len(offsets)+1)
	for _, off := range offsets {
		fmt.Fprintf(&b, "%010d 00000 n \n", off)
	}
	fmt.Fprintf(&b, "trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n", len(offsets)+1, xref)
	return b.Bytes()
}

// bombPDF is a PDF of one page whose content stream is one string of the given number of mebibytes, written
// as the text of the page: a few kilobytes of file that the reader has to hold as many megabytes of.
func bombPDF(mib int) []byte {
	var packed bytes.Buffer
	zw, _ := zlib.NewWriterLevel(&packed, zlib.BestSpeed)
	_, _ = zw.Write([]byte("BT /F1 12 Tf ("))
	block := bytes.Repeat([]byte("a"), 1<<20)
	for i := 0; i < mib; i++ {
		_, _ = zw.Write(block)
	}
	_, _ = zw.Write([]byte(") Tj ET"))
	_ = zw.Close()

	var b bytes.Buffer
	var offsets []int
	obj := func(body string) {
		offsets = append(offsets, b.Len())
		fmt.Fprintf(&b, "%d 0 obj\n%s\nendobj\n", len(offsets), body)
	}
	b.WriteString("%PDF-1.4\n")
	obj("<< /Type /Catalog /Pages 2 0 R >>")
	obj("<< /Type /Pages /Kids [4 0 R] /Count 1 >>")
	obj("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
	obj("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>")
	offsets = append(offsets, b.Len())
	fmt.Fprintf(&b, "%d 0 obj\n<< /Length %d /Filter /FlateDecode >>\nstream\n", len(offsets), packed.Len())
	b.Write(packed.Bytes())
	b.WriteString("\nendstream\nendobj\n")
	xref := b.Len()
	fmt.Fprintf(&b, "xref\n0 %d\n0000000000 65535 f \n", len(offsets)+1)
	for _, off := range offsets {
		fmt.Fprintf(&b, "%010d 00000 n \n", off)
	}
	fmt.Fprintf(&b, "trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n", len(offsets)+1, xref)
	return b.Bytes()
}

// winAnsi escapes a string for a PDF literal string and maps the Norwegian letters to WinAnsi.
func winAnsi(s string) string {
	r := strings.NewReplacer(`\`, `\\`, `(`, `\(`, `)`, `\)`,
		"æ", "\xe6", "ø", "\xf8", "å", "\xe5", "Æ", "\xc6", "Ø", "\xd8", "Å", "\xc5", "\u00a0", "\xa0")
	return r.Replace(s)
}

// docxFile builds a DOCX whose body is the given WordprocessingML.
func docxFile(t *testing.T, body string) []byte {
	t.Helper()
	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	add := func(name, data string) {
		w, err := zw.Create(name)
		if err != nil {
			t.Fatal(err)
		}
		_, _ = w.Write([]byte(data))
	}
	add("[Content_Types].xml", `<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>`)
	add("_rels/.rels", `<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>`)
	add("word/document.xml", `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>`+body+`</w:body></w:document>`)
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes()
}

// para is a WordprocessingML paragraph with one run of text.
func para(text string) string {
	return `<w:p><w:r><w:t xml:space="preserve">` + text + `</w:t></w:r></w:p>`
}
