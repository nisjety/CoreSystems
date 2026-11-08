#!/usr/bin/env python3
"""Generates the document parsing and PII fixture corpus (E3.6-10).

Run from this directory:  python generate.py

Writes no/*.pdf, no/*.docx and corpus.json. Every file is SYNTHETIC test data:
the names, case numbers and addresses are invented; the identity numbers pass
the mod-11 control but belong to no person; the bank account and card numbers
are published examples (the IBAN and card checks are run below as well). The
output is deterministic (fixed PDF layout, fixed zip timestamps, fixed scan
rendering), so running this again leaves the bytes unchanged.

The ground truth in corpus.json is what a correct reader must find: the PII
values a detector must redact, the near-miss strings it must leave alone, the
classification of each PDF page and the text a scan carries (for OCR later).
"""

import hashlib
import io
import json
import os
import sys
import zipfile
import zlib
from xml.sax.saxutils import escape as xml_escape

from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
PAGE_W, PAGE_H = 595, 842
SCAN_W, SCAN_H = 1000, 1414
FONT_STYLES = {"title": (13, "F2"), "body": (10.5, "F1"), "small": (8, "F1")}

# ---------------------------------------------------------------------------
# Checks the ground truth must pass (the same rules the Rust validators apply).
# ---------------------------------------------------------------------------


def fnr_valid(digits):
    d = [int(c) for c in digits]
    if len(d) != 11:
        return False
    k1 = [3, 7, 6, 1, 8, 9, 4, 5, 2]
    k2 = [5, 4, 3, 2, 7, 6, 5, 4, 3, 2]

    def control(weights, values):
        total = sum(w * v for w, v in zip(weights, values))
        r = 11 - total % 11
        return 0 if r == 11 else r

    c1 = control(k1, d[:9])
    c2 = control(k2, d[:10])
    return c1 != 10 and c2 != 10 and d[9] == c1 and d[10] == c2


def iban_valid(text):
    s = text.replace(" ", "")
    rearranged = s[4:] + s[:4]
    numeric = "".join(str(int(ch, 36)) for ch in rearranged)
    return int(numeric) % 97 == 1


def luhn_valid(text):
    digits = [int(c) for c in text if c.isdigit()]
    total = 0
    for i, d in enumerate(reversed(digits)):
        if i % 2 == 1:
            d *= 2
            if d > 9:
                d -= 9
        total += d
    return total % 10 == 0 and len(digits) >= 13


def phone_valid(text):
    intl = text.startswith("+")
    digits = "".join(c for c in text if c.isdigit())
    if intl:
        return 8 <= len(digits) <= 15 and digits[0] != "0"
    return len(digits) == 8 and digits[0] in "23456789"


CHECKS = {
    "fodselsnummer": lambda v: fnr_valid("".join(c for c in v if c.isdigit())),
    "iban": iban_valid,
    "card": luhn_valid,
    "phone": phone_valid,
    "email": lambda v: "@" in v and "." in v.split("@")[1],
}

# ---------------------------------------------------------------------------
# The ten Norwegian public-sector documents. Lines are (style, text).
# ---------------------------------------------------------------------------

MARK = "SYNTETISK TESTDOKUMENT"

DOCS = [
    {
        "id": "no-01-vedtak-bostotte",
        "file": "no/no-01-vedtak-bostotte.pdf",
        "format": "pdf",
        "summary": "Text PDF, one page. Decision letter: national identity number, email, international phone, Norwegian IBAN. Near misses: case number, postal code.",
        "pages": [
            {"type": "text", "lines": [
                ("title", "VEDTAK OM BOSTØTTE (" + MARK + ")"),
                ("body", "Testkommune Nord, bostøtteavdelingen"),
                ("body", ""),
                ("body", "Saksnummer: 2024/104532"),
                ("body", "Dato: 14. mars 2024"),
                ("body", ""),
                ("body", "Søker: Kari Testperson"),
                ("body", "Fødselsnummer: 12058512336"),
                ("body", "E-post: kari.testperson@example.no"),
                ("body", "Telefon: +47 22 33 44 55"),
                ("body", "Utbetaling til konto: NO93 8601 1117 947"),
                ("body", ""),
                ("body", "Vedtaket er fattet etter husbankens regelverk. Klage må sendes"),
                ("body", "innen tre uker fra den dagen du mottok vedtaket."),
                ("body", "Postadresse: Rådhusveien 1, 0150 Oslo."),
            ]},
        ],
        "pii": [
            ("fodselsnummer", "12058512336"),
            ("email", "kari.testperson@example.no"),
            ("phone", "+47 22 33 44 55"),
            ("iban", "NO93 8601 1117 947"),
        ],
        "near_miss": ["2024/104532", "0150 Oslo"],
    },
    {
        "id": "no-02-soknad-barnehageplass",
        "file": "no/no-02-soknad-barnehageplass.pdf",
        "format": "pdf",
        "summary": "Text PDF, two pages. Application form: two identity numbers, a national mobile number, an email and a payment card. Near miss: application number.",
        "pages": [
            {"type": "text", "lines": [
                ("title", "SØKNAD OM BARNEHAGEPLASS (" + MARK + ")"),
                ("body", "Del 1: Søkerens opplysninger"),
                ("body", ""),
                ("body", "Søkerens navn: Ola Testmann"),
                ("body", "Fødselsnummer: 13058512375"),
                ("body", "Telefon mobil: 41 23 45 67"),
                ("body", "E-post: ola.testmann@example.no"),
                ("body", "Søknadsnummer: 2024-55871"),
                ("small", "Side 1 av 2"),
            ]},
            {"type": "text", "lines": [
                ("title", "SØKNAD OM BARNEHAGEPLASS, SIDE 2"),
                ("body", "Del 2: Barnets opplysninger"),
                ("body", ""),
                ("body", "Barnets fødselsnummer: 01010078950"),
                ("body", "Ønsket oppstart: 1. august 2024"),
                ("body", "Betaling av oppholdsbetaling med kort: 4111 1111 1111 1111"),
                ("small", "Side 2 av 2"),
            ]},
        ],
        "pii": [
            ("fodselsnummer", "13058512375"),
            ("phone", "41 23 45 67"),
            ("email", "ola.testmann@example.no"),
            ("fodselsnummer", "01010078950"),
            ("card", "4111 1111 1111 1111"),
        ],
        "near_miss": ["2024-55871"],
    },
    {
        "id": "no-03-faktura-kort",
        "file": "no/no-03-faktura-kort.pdf",
        "format": "pdf",
        "summary": "Text PDF, one page. Invoice: IBAN, an email, a published test card number. Near misses: organisation number, KID, a card-shaped order number that fails Luhn.",
        "pages": [
            {"type": "text", "lines": [
                ("title", "FAKTURA (" + MARK + ")"),
                ("body", "Fakturanummer: 2024-0917"),
                ("body", "Fakturadato: 2. april 2024"),
                ("body", "Forfallsdato: 2. mai 2024"),
                ("body", ""),
                ("body", "Kunde: Testbedriften AS, org.nr 974 760 673"),
                ("body", "Kontaktperson: Kari Testperson, kari.testperson@example.no"),
                ("body", "Beløp: 12 500,00 NOK"),
                ("body", "KID: 1234567890"),
                ("body", "Bankkonto: NO93 8601 1117 947"),
                ("body", "Betalt med kort: 5555 5555 5555 4444"),
                ("body", "Ordrenummer: 4111 1111 1111 1112"),
            ]},
        ],
        "pii": [
            ("email", "kari.testperson@example.no"),
            ("iban", "NO93 8601 1117 947"),
            ("card", "5555 5555 5555 4444"),
        ],
        "near_miss": ["974 760 673", "1234567890", "4111 1111 1111 1112", "2024-0917"],
    },
    {
        "id": "no-04-skannet-innkalling",
        "file": "no/no-04-skannet-innkalling.pdf",
        "format": "pdf",
        "summary": "Scanned PDF, one page: an image with no text layer. The OCR truth is listed. Holds a national H-number and an international phone.",
        "pages": [
            {"type": "scan", "lines": [
                ("title", "INNKALLING TIL MØTE I RÅDET"),
                ("body", "Testkommune Nord, sekretariatet"),
                ("body", ""),
                ("body", "Dato: 12. april 2024, kl. 09.00"),
                ("body", "Sted: Rådhussalen, 3. etasje"),
                ("body", "Mottaker: Per Testsen"),
                ("body", "Fødselsnummer: 12458512319"),
                ("body", "Telefon: +47 22 33 44 55"),
                ("body", "Saksnummer: 2024/104532"),
                ("body", ""),
                ("body", "Vennligst gi beskjed om forfall til postmottak"),
                ("body", "senest 5. april."),
            ]},
        ],
        "pii": [
            ("fodselsnummer", "12458512319"),
            ("phone", "+47 22 33 44 55"),
        ],
        "near_miss": ["2024/104532"],
    },
    {
        "id": "no-05-blandet-notat-og-vedlegg",
        "file": "no/no-05-blandet-notat-og-vedlegg.pdf",
        "format": "pdf",
        "summary": "Mixed PDF, two pages: page 1 has a text layer, page 2 is a scan. Tests the per-page classifier on one document.",
        "pages": [
            {"type": "text", "lines": [
                ("title", "INTERNT NOTAT (" + MARK + ")"),
                ("body", ""),
                ("body", "Til: Saksbehandler Eva Testdal"),
                ("body", "Fra: Sekretariatet, Testkommune Nord"),
                ("body", "Gjelder: vedlegg til innkalling, side 2 er skannet"),
                ("body", ""),
                ("body", "Kontakt for spørsmål: +47 22 33 44 55"),
                ("body", "E-post: eva.testdal@example.no"),
            ]},
            {"type": "scan", "lines": [
                ("title", "VEDLEGG: ANSATTLISTE (SKANNET)"),
                ("body", ""),
                ("body", "1. Per Testsen, fødselsnummer 01010078950"),
                ("body", "2. Kari Testperson, telefon 41 23 45 67"),
            ]},
        ],
        "pii": [
            ("phone", "+47 22 33 44 55"),
            ("email", "eva.testdal@example.no"),
            ("fodselsnummer", "01010078950"),
            ("phone", "41 23 45 67"),
        ],
        "near_miss": [],
    },
    {
        "id": "no-06-skann-kvittering-sidefot",
        "file": "no/no-06-skann-kvittering-sidefot.pdf",
        "format": "pdf",
        "summary": "Scanned PDF, one page with a short vector footer over the scan. The footer is under the text threshold, so the page is a scan. Holds a published test card number.",
        "pages": [
            {"type": "scan", "footer": "Side 1 av 1", "lines": [
                ("title", "KVITTERING (" + MARK + ")"),
                ("body", "Testkiosken, Storgata 1, 0155 Oslo"),
                ("body", ""),
                ("body", "Vare 1: Kaffe 49,00"),
                ("body", "Vare 2: Kanelbolle 32,00"),
                ("body", "Totalt NOK 81,00"),
                ("body", ""),
                ("body", "Betalt med kort 4111 1111 1111 1111"),
                ("body", "Kortholder: Kari Testperson"),
            ]},
        ],
        "pii": [
            ("card", "4111 1111 1111 1111"),
        ],
        "near_miss": ["0155 Oslo"],
    },
    {
        "id": "no-07-nare-treff",
        "file": "no/no-07-nare-treff.pdf",
        "format": "pdf",
        "summary": "Text PDF, one page with no personal data. Every line is a near miss: case, order and organisation numbers, a UUID, a git SHA, postal codes and a version. Must produce no findings.",
        "pages": [
            {"type": "text", "lines": [
                ("title", "NÆRE TREFF (" + MARK + ", SKAL IKKE SENSURERES)"),
                ("body", ""),
                ("body", "Saksnummer: 2024/104532"),
                ("body", "Ordrenummer: 4111 1111 1111 1112"),
                ("body", "Organisasjonsnummer: 974 760 673"),
                ("body", "Postnummer: 0150 Oslo, 0155 Oslo"),
                ("body", "Forsendelses-id: 3f2504e0-4f89-11d3-9a0c-0305e82c3301"),
                ("body", "Versjon 2.14.0, bygg 3f9a1c7e0b5d2a84f61e9c03b7d5a2e8c1f40b96"),
                ("body", "Dato: 12.04.2024, referanse A-12/2024"),
            ]},
        ],
        "pii": [],
        "near_miss": [
            "2024/104532",
            "4111 1111 1111 1112",
            "974 760 673",
            "0150 Oslo",
            "3f2504e0-4f89-11d3-9a0c-0305e82c3301",
            "12.04.2024",
        ],
    },
    {
        "id": "no-08-vedtak-tilskudd",
        "file": "no/no-08-vedtak-tilskudd.docx",
        "format": "docx",
        "summary": "DOCX, paragraphs only. Grant decision with an email, a national identity number, a national mobile number in +47 form and an IBAN. Near miss: case number.",
        "blocks": [
            {"type": "p", "text": "Vedtak om tilskudd (" + MARK + ")", "bold": True},
            {"type": "p", "text": "Dato: 20. februar 2024"},
            {"type": "p", "text": "Mottaker: Per Testsen, e-post per.testsen@example.no"},
            {"type": "p", "text": "Fødselsnummer: 31129900183"},
            {"type": "p", "text": "Telefon: +47 91 23 45 67"},
            {"type": "p", "text": "Tilskuddet på kr 15 000 utbetales til konto NO93 8601 1117 947 innen 14 dager."},
            {"type": "p", "text": "Saksnummer: 2024/77412. Vedtaket kan påklages innen tre uker."},
        ],
        "pii": [
            ("email", "per.testsen@example.no"),
            ("fodselsnummer", "31129900183"),
            ("phone", "+47 91 23 45 67"),
            ("iban", "NO93 8601 1117 947"),
        ],
        "near_miss": ["2024/77412"],
    },
    {
        "id": "no-09-oppdragsavtale-tabell",
        "file": "no/no-09-oppdragsavtale-tabell.docx",
        "format": "docx",
        "summary": "DOCX with a table. Engagement agreement: a D-number, a national identity number, two IBANs (one Norwegian, one German published example) and a phone. Near miss: contract number.",
        "blocks": [
            {"type": "p", "text": "Oppdragsavtale (" + MARK + ")", "bold": True},
            {"type": "p", "text": "Kontraktsnummer: 2023-0042"},
            {"type": "table", "rows": [
                ["Navn", "Fødselsnummer eller D-nummer", "Kontonummer"],
                ["Ola Testmann", "52058512400", "NO93 8601 1117 947"],
                ["Eva Testdal", "12058512336", "DE89 3704 0044 0532 0130 00"],
            ]},
            {"type": "p", "text": "Kontakt: +47 91 23 45 67. Oppdragstaker har lest og godtatt vilkårene."},
        ],
        "pii": [
            ("fodselsnummer", "52058512400"),
            ("iban", "NO93 8601 1117 947"),
            ("fodselsnummer", "12058512336"),
            ("iban", "DE89 3704 0044 0532 0130 00"),
            ("phone", "+47 91 23 45 67"),
        ],
        "near_miss": ["2023-0042"],
    },
    {
        "id": "no-10-journalnotat",
        "file": "no/no-10-journalnotat.pdf",
        "format": "pdf",
        "summary": "Text PDF, two pages. Journal note: an identity number written with a space, a national phone, an email on page 2. Near misses: case and journal numbers.",
        "pages": [
            {"type": "text", "lines": [
                ("title", "JOURNALNOTAT (" + MARK + ")"),
                ("body", "Sak: 2024/1183. Jnr. 24/00912."),
                ("body", "Journalført: 3. mai 2024 av saksbehandler Eva Testdal"),
                ("body", ""),
                ("body", "Innbygger: Kari Testperson"),
                ("body", "Fødselsnummer: 120585 12336"),
                ("body", ""),
                ("body", "Henvendelsen gjelder spørsmål om utsettelse av frist for dokumentasjon."),
                ("body", "Innbygger ringte fra telefon 22 33 44 55 og ba om svar skriftlig."),
                ("small", "Side 1 av 2"),
            ]},
            {"type": "text", "lines": [
                ("title", "JOURNALNOTAT, SIDE 2"),
                ("body", "Notatet fortsetter. Svar sendes til e-post"),
                ("body", "kari.testperson@example.no etter godkjenning."),
                ("body", "Vedtak fattes innen 14 dager. Frist for ny dokumentasjon"),
                ("body", "settes til 1. juni 2024."),
                ("small", "Side 2 av 2"),
            ]},
        ],
        "pii": [
            ("fodselsnummer", "120585 12336"),
            ("phone", "22 33 44 55"),
            ("email", "kari.testperson@example.no"),
        ],
        "near_miss": ["2024/1183", "24/00912"],
    },
]

# ---------------------------------------------------------------------------
# PDF writer (hand-built, so the bytes and the xref are fixed by this script).
# ---------------------------------------------------------------------------


def pdf_string(text):
    out = bytearray(b"(")
    for b in text.encode("cp1252"):
        if b in (0x28, 0x29, 0x5C):
            out.append(0x5C)
        out.append(b)
    out += b")"
    return bytes(out)


def text_content(lines):
    out = [b"BT"]
    y = PAGE_H - 70.0
    for style, text in lines:
        size, font = FONT_STYLES[style]
        if text:
            out.append(
                b"/%s %s Tf 1 0 0 1 56 %.2f Tm %s Tj"
                % (font.encode(), repr(size).encode(), y, pdf_string(text))
            )
        y -= size * 1.5
    out.append(b"ET")
    return b"\n".join(out) + b"\n"


def scan_content(footer):
    out = b"q %.3f 0 0 %.3f 0 0 cm /Im1 Do Q\n" % (PAGE_W, PAGE_H)
    if footer:
        out += b"BT /F1 8 Tf 1 0 0 1 56 30 Tm %s Tj ET\n" % pdf_string(footer)
    return out


def render_scan(lines):
    """Draws the lines as a grey raster, the way a scanner's output looks."""
    img = Image.new("L", (SCAN_W, SCAN_H), 248)
    draw = ImageDraw.Draw(img)
    body_font = ImageFont.load_default(size=26)
    title_font = ImageFont.load_default(size=36)
    draw.rectangle([40, 40, SCAN_W - 40, SCAN_H - 40], outline=200, width=2)
    y = 110
    for style, text in lines:
        font = title_font if style == "title" else body_font
        if text:
            draw.text((90, y), text, fill=20, font=font)
        y += 60 if style == "title" else 44
    return img


def build_pdf(pages):
    objs = {}
    counter = [0]

    def new_id():
        counter[0] += 1
        return counter[0]

    catalog = new_id()
    pages_id = new_id()
    f1 = new_id()
    f2 = new_id()
    objs[f1] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
    objs[f2] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
    kids = []
    for page in pages:
        page_id = new_id()
        content_id = new_id()
        xobject = b""
        if page["type"] == "text":
            content = text_content(page["lines"])
        else:
            image_id = new_id()
            raw = render_scan(page["lines"]).tobytes()
            comp = zlib.compress(raw, 9)
            objs[image_id] = (
                b"<< /Type /XObject /Subtype /Image /Width %d /Height %d "
                b"/ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length %d >>\nstream\n"
                % (SCAN_W, SCAN_H, len(comp))
            ) + comp + b"\nendstream"
            content = scan_content(page.get("footer"))
            xobject = b" /XObject << /Im1 %d 0 R >>" % image_id
        objs[content_id] = b"<< /Length %d >>\nstream\n" % len(content) + content + b"endstream"
        objs[page_id] = (
            b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d] "
            b"/Resources << /Font << /F1 %d 0 R /F2 %d 0 R >>%s >> /Contents %d 0 R >>"
            % (pages_id, PAGE_W, PAGE_H, f1, f2, xobject, content_id)
        )
        kids.append(page_id)
    objs[pages_id] = b"<< /Type /Pages /Kids [%s] /Count %d >>" % (
        b" ".join(b"%d 0 R" % k for k in kids),
        len(kids),
    )
    objs[catalog] = b"<< /Type /Catalog /Pages %d 0 R >>" % pages_id

    out = bytearray(b"%PDF-1.5\n%\xe2\xe3\xcf\xd3\n")
    offsets = {}
    for num in sorted(objs):
        offsets[num] = len(out)
        out += b"%d 0 obj\n" % num + objs[num] + b"\nendobj\n"
    xref_at = len(out)
    size = max(objs) + 1
    out += b"xref\n0 %d\n" % size
    out += b"0000000000 65535 f \n"
    for num in range(1, size):
        out += b"%010d 00000 n \n" % offsets[num]
    out += b"trailer\n<< /Size %d /Root %d 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        size,
        catalog,
        xref_at,
    )
    return bytes(out)


# ---------------------------------------------------------------------------
# DOCX writer (WordprocessingML in a deflated zip, fixed timestamps).
# ---------------------------------------------------------------------------


def para_xml(text, bold=False):
    rpr = "<w:rPr><w:b/></w:rPr>" if bold else ""
    return '<w:p><w:r>%s<w:t xml:space="preserve">%s</w:t></w:r></w:p>' % (
        rpr,
        xml_escape(text),
    )


def document_xml(blocks):
    parts = []
    for block in blocks:
        if block["type"] == "p":
            parts.append(para_xml(block["text"], block.get("bold", False)))
        else:
            rows = block["rows"]
            width = len(rows[0])
            grid = '<w:gridCol w:w="2400"/>' * width
            trs = []
            for index, row in enumerate(rows):
                cells = "".join(
                    '<w:tc><w:tcPr><w:tcW w:w="2400" w:type="dxa"/></w:tcPr>%s</w:tc>'
                    % para_xml(cell, bold=index == 0)
                    for cell in row
                )
                trs.append("<w:tr>%s</w:tr>" % cells)
            parts.append(
                '<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr>'
                "<w:tblGrid>%s</w:tblGrid>%s</w:tbl>" % (grid, "".join(trs))
            )
    return (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">'
        "<w:body>%s</w:body></w:document>" % "".join(parts)
    ).encode("utf-8")


CONTENT_TYPES = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
    '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
    '<Default Extension="xml" ContentType="application/xml"/>'
    '<Override PartName="/word/document.xml" '
    'ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
    "</Types>"
).encode("utf-8")

ROOT_RELS = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    '<Relationship Id="rId1" '
    'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" '
    'Target="word/document.xml"/></Relationships>'
).encode("utf-8")


def build_docx(blocks):
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name, data in (
            ("[Content_Types].xml", CONTENT_TYPES),
            ("_rels/.rels", ROOT_RELS),
            ("word/document.xml", document_xml(blocks)),
        ):
            info = zipfile.ZipInfo(name, date_time=(2024, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data)
    return stream.getvalue()


# ---------------------------------------------------------------------------
# Corpus output
# ---------------------------------------------------------------------------


def pdf_page_summary(doc):
    return [
        {"page": index + 1, "kind": "text" if page["type"] == "text" else "scanned"}
        for index, page in enumerate(doc["pages"])
    ]


def ocr_truth(doc):
    lines = []
    for page in doc.get("pages", []):
        if page["type"] == "scan":
            lines.extend(text for _, text in page["lines"] if text)
    return lines


def main():
    ok = True
    manifest_docs = []
    for doc in DOCS:
        for kind, value in doc["pii"]:
            check = CHECKS.get(kind)
            if check is None or not check(value):
                print("GROUND TRUTH FAILS ITS CHECK:", doc["id"], kind, value, file=sys.stderr)
                ok = False
    if not ok:
        return 1

    for doc in DOCS:
        path = os.path.join(HERE, doc["file"])
        os.makedirs(os.path.dirname(path), exist_ok=True)
        if doc["format"] == "pdf":
            data = build_pdf(doc["pages"])
        else:
            data = build_docx(doc["blocks"])
        with open(path, "wb") as handle:
            handle.write(data)
        entry = {
            "id": doc["id"],
            "file": doc["file"],
            "format": doc["format"],
            "language": "nb",
            "summary": doc["summary"],
            "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
            "pii": [{"kind": k, "value": v} for k, v in doc["pii"]],
            "near_miss": doc["near_miss"],
        }
        if doc["format"] == "pdf":
            entry["pages"] = pdf_page_summary(doc)
            entry["ocr_truth"] = ocr_truth(doc)
        manifest_docs.append(entry)

    manifest = {
        "schema": "quarry-document-corpus/1",
        "note": (
            "Synthetic Norwegian public-sector fixture set for document parsing and PII "
            "(E3.6-10). Invented people, cases and addresses; identity numbers pass the "
            "mod-11 control but belong to no one; IBAN and card numbers are published "
            "examples. Regenerate with generate.py; do not hand-edit corpus.json."
        ),
        "documents": manifest_docs,
    }
    with open(os.path.join(HERE, "corpus.json"), "w", encoding="utf-8", newline="\n") as handle:
        json.dump(manifest, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    print("wrote %d documents and corpus.json" % len(manifest_docs))
    return 0


if __name__ == "__main__":
    sys.exit(main())
