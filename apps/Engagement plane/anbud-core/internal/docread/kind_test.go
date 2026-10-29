package docread

import (
	"archive/zip"
	"bytes"
	"testing"
)

func zipWith(t *testing.T, names ...string) []byte {
	t.Helper()
	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	for _, n := range names {
		w, err := zw.Create(n)
		if err != nil {
			t.Fatal(err)
		}
		_, _ = w.Write([]byte("x"))
	}
	_ = zw.Close()
	return buf.Bytes()
}

// What a file is, is decided from its bytes. The name is the sender's word: a PDF called .txt is a PDF.
func TestDetectDecidesFromTheBytes(t *testing.T) {
	t.Parallel()

	utf16 := append([]byte{0xFF, 0xFE}, []byte{'H', 0, 'e', 0, 'i', 0}...)
	for name, tc := range map[string]struct {
		data []byte
		want Kind
	}{
		"pdf":                          {[]byte("%PDF-1.7\n1 0 obj"), KindPDF},
		"pdf after a few junk bytes":   {append([]byte("\r\n\r\n   "), []byte("%PDF-1.4")...), KindPDF},
		"docx":                         {docxFile(t, para("x")), KindDOCX},
		"xlsx":                         {zipWith(t, "[Content_Types].xml", "xl/workbook.xml"), KindXLSX},
		"pptx":                         {zipWith(t, "[Content_Types].xml", "ppt/presentation.xml"), KindPPTX},
		"a zip of other things":        {zipWith(t, "a.txt", "b.txt"), KindArchive},
		"legacy office":                {[]byte{0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0}, KindOLE},
		"rtf":                          {[]byte(`{\rtf1\ansi Hei}`), KindRTF},
		"png":                          {[]byte("\x89PNG\r\n\x1a\n...."), KindImage},
		"jpeg":                         {[]byte{0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10}, KindImage},
		"gif":                          {[]byte("GIF89a...."), KindImage},
		"tiff little endian":           {[]byte{'I', 'I', '*', 0, 8, 0}, KindImage},
		"tiff big endian":              {[]byte{'M', 'M', 0, '*', 0, 8}, KindImage},
		"7z":                           {[]byte{'7', 'z', 0xBC, 0xAF, 0x27, 0x1C, 0}, KindArchive},
		"rar":                          {[]byte("Rar!\x1a\x07\x00"), KindArchive},
		"gzip":                         {[]byte{0x1F, 0x8B, 8, 0, 0}, KindArchive},
		"html":                         {[]byte("<!DOCTYPE html><html><body>Hei</body></html>"), KindHTML},
		"html with a BOM and capitals": {append([]byte{0xEF, 0xBB, 0xBF}, []byte("  \n<HTML><HEAD>")...), KindHTML},
		"xml":                          {[]byte(`<?xml version="1.0"?><espd/>`), KindXML},
		"utf-8 text":                   {[]byte("Leverandøren skal ha ISO 9001.\n"), KindText},
		"windows-1252 text":            {[]byte("Bl\xe5b\xe6r og \xf8l"), KindText},
		"utf-16 text":                  {utf16, KindText},
		"binary":                       {[]byte{0, 1, 2, 3, 0, 255, 254, 0, 9}, KindOther},
		"empty":                        {nil, KindOther},
	} {
		if got := Detect(tc.data); got != tc.want {
			t.Errorf("%s: Detect = %q, want %q", name, got, tc.want)
		}
	}
}

func TestDetectDoesNotTakeTextThatMentionsAFormatForThatFormat(t *testing.T) {
	t.Parallel()

	for name, data := range map[string][]byte{
		"a sentence about PDF":  []byte("Vedlegget leveres som %PDF-1.4 fil."),
		"a sentence about html": []byte("Bruk taggen <html> i malen, sier han, og så videre."),
	} {
		if got := Detect(data); got != KindText {
			t.Errorf("%s: Detect = %q, want text", name, got)
		}
	}
}
