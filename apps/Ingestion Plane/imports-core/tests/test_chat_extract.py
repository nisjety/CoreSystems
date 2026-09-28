import base64
import io
import zipfile
import pytest
from docx import Document
from fastapi import HTTPException
from app.chat_extract import ChatExtractRequest, extract_chat_document


def request(name, content):
    return ChatExtractRequest(filename=name, content_base64=base64.b64encode(content).decode())


# Hand-built minimal OOXML zips: no openpyxl/python-pptx dependency exists in
# this service (nor does app.parsers use one), so the fixtures below assemble
# just the zip parts app.parsers._parse_xlsx / _parse_pptx actually read.

def _build_xlsx(sheets: list[tuple[str, list[list[str]]]]) -> bytes:
    shared: list[str] = []

    def shared_index(text: str) -> int:
        if text not in shared:
            shared.append(text)
        return shared.index(text)

    sheet_entries = []
    worksheet_files = {}
    for sheet_index, (name, rows) in enumerate(sheets, start=1):
        row_xml = []
        for row_index, row in enumerate(rows, start=1):
            cells_xml = []
            for col_index, value in enumerate(row):
                col_letter = chr(ord('A') + col_index)
                ref = f'{col_letter}{row_index}'
                cells_xml.append(f'<c r="{ref}" t="s"><v>{shared_index(value)}</v></c>')
            row_xml.append(f'<row r="{row_index}">{"".join(cells_xml)}</row>')
        worksheet_files[f'xl/worksheets/sheet{sheet_index}.xml'] = (
            '<?xml version="1.0"?>'
            '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
            f'<sheetData>{"".join(row_xml)}</sheetData></worksheet>'
        )
        sheet_entries.append(
            f'<sheet name="{name}" sheetId="{sheet_index}" r:id="rId{sheet_index}"/>'
        )

    workbook_xml = (
        '<?xml version="1.0"?>'
        '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
        'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
        f'<sheets>{"".join(sheet_entries)}</sheets></workbook>'
    )
    rels_xml = (
        '<?xml version="1.0"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        + ''.join(
            f'<Relationship Id="rId{i}" Target="worksheets/sheet{i}.xml"/>'
            for i in range(1, len(sheets) + 1)
        )
        + '</Relationships>'
    )
    shared_strings_xml = (
        '<?xml version="1.0"?>'
        f'<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{len(shared)}" uniqueCount="{len(shared)}">'
        + ''.join(f'<si><t>{text}</t></si>' for text in shared)
        + '</sst>'
    )

    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, 'w') as zf:
        zf.writestr('xl/workbook.xml', workbook_xml)
        zf.writestr('xl/_rels/workbook.xml.rels', rels_xml)
        zf.writestr('xl/sharedStrings.xml', shared_strings_xml)
        for path, xml in worksheet_files.items():
            zf.writestr(path, xml)
    return buffer.getvalue()


def _build_pptx(slides: list[list[str]]) -> bytes:
    sld_id_list = []
    rels = []
    slide_files = {}
    for index, paragraphs in enumerate(slides, start=1):
        rid = f'rId{index + 1}'
        sld_id_list.append(f'<p:sldId id="{255 + index}" r:id="{rid}"/>')
        rels.append(f'<Relationship Id="{rid}" Target="slides/slide{index}.xml"/>')
        paragraph_xml = ''.join(
            f'<a:p><a:r><a:t>{text}</a:t></a:r></a:p>' for text in paragraphs
        )
        slide_files[f'ppt/slides/slide{index}.xml'] = (
            '<?xml version="1.0"?>'
            '<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" '
            'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">'
            f'<p:cSld><p:spTree><p:sp><p:txBody>{paragraph_xml}</p:txBody></p:sp></p:spTree></p:cSld></p:sld>'
        )

    presentation_xml = (
        '<?xml version="1.0"?>'
        '<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" '
        'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
        f'<p:sldIdLst>{"".join(sld_id_list)}</p:sldIdLst></p:presentation>'
    )
    rels_xml = (
        '<?xml version="1.0"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        + ''.join(rels)
        + '</Relationships>'
    )

    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, 'w') as zf:
        zf.writestr('ppt/presentation.xml', presentation_xml)
        zf.writestr('ppt/_rels/presentation.xml.rels', rels_xml)
        for path, xml in slide_files.items():
            zf.writestr(path, xml)
    return buffer.getvalue()


@pytest.mark.parametrize('name', ['source.md', 'sales.csv'])
def test_text_extraction_is_complete_and_ephemeral(name):
    result = extract_chat_document(request(name, b'first\nlast'))
    assert result['data']['content'] == 'first\nlast'
    assert result['data']['persisted'] is False


def test_docx_extraction():
    document = Document()
    document.add_paragraph('Order FF-1042: 9 packed, 3 pending.')
    table = document.add_table(rows=1, cols=2)
    table.cell(0, 0).text = 'Current policy'
    table.cell(0, 1).text = 'K3'
    document.add_paragraph('Last paragraph')
    buffer = io.BytesIO()
    document.save(buffer)
    text = extract_chat_document(request('order.docx', buffer.getvalue()))['data']['content']
    assert '9 packed, 3 pending' in text
    assert text.index('9 packed') < text.index('Current policy\tK3') < text.index('Last paragraph')


def test_xlsx_extraction_keeps_sheet_order_and_cell_values():
    content = _build_xlsx([
        ('Orders', [['Order', 'Qty'], ['FF-1042', '9']]),
        ('Notes', [['Last row']]),
    ])
    text = extract_chat_document(request('report.xlsx', content))['data']['content']
    assert text.index('[Orders]') < text.index('FF-1042\t9') < text.index('[Notes]') < text.index('Last row')


def test_pptx_extraction_keeps_slide_order_and_text():
    content = _build_pptx([
        ['Title One', 'Bullet A'],
        ['Title Two'],
    ])
    text = extract_chat_document(request('deck.pptx', content))['data']['content']
    assert text.index('[Slide 1]') < text.index('Title One') < text.index('Bullet A') < text.index('[Slide 2]') < text.index('Title Two')


@pytest.mark.parametrize('name,content,status', [
    ('broken.pdf', b'not a PDF', 422), ('empty.md', b' ', 422),
    ('large.md', b'x' * 60_001, 413), ('unknown.exe', b'binary', 415),
    ('broken.xlsx', b'not a zip', 422), ('broken.pptx', b'not a zip', 422),
])
def test_unreadable_or_oversized_input_is_rejected(name, content, status):
    with pytest.raises(HTTPException) as exc:
        extract_chat_document(request(name, content))
    assert exc.value.status_code == status


# OPC relationship targets are normally relative to the referencing part's own
# folder ("worksheets/sheet1.xml", "slides/slide1.xml"), but the spec also
# allows a package-root-absolute form ("/xl/worksheets/sheet1.xml"). Real
# Excel/PowerPoint output never uses the absolute form, but nothing rules it
# out, and _xlsx_sheet_parts/_pptx_slide_parts must not silently read the
# sheet/slide as empty when a producer does.

def test_xlsx_with_absolute_style_relationship_target_still_reads_sheet():
    workbook_xml = (
        '<?xml version="1.0"?>'
        '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
        'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
        '<sheets><sheet name="Orders" sheetId="1" r:id="rId1"/></sheets></workbook>'
    )
    rels_xml = (
        '<?xml version="1.0"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Target="/xl/worksheets/sheet1.xml"/></Relationships>'
    )
    sheet_xml = (
        '<?xml version="1.0"?>'
        '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
        '<sheetData><row r="1"><c r="A1"><v>9</v></c></row></sheetData></worksheet>'
    )
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, 'w') as zf:
        zf.writestr('xl/workbook.xml', workbook_xml)
        zf.writestr('xl/_rels/workbook.xml.rels', rels_xml)
        zf.writestr('xl/worksheets/sheet1.xml', sheet_xml)
    text = extract_chat_document(request('report.xlsx', buffer.getvalue()))['data']['content']
    assert text == '[Orders]\n9'


def test_pptx_with_absolute_style_relationship_target_still_reads_slide():
    presentation_xml = (
        '<?xml version="1.0"?>'
        '<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" '
        'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
        '<p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>'
    )
    rels_xml = (
        '<?xml version="1.0"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Target="/ppt/slides/slide1.xml"/></Relationships>'
    )
    slide_xml = (
        '<?xml version="1.0"?>'
        '<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" '
        'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">'
        '<p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Title</a:t></a:r></a:p>'
        '</p:txBody></p:sp></p:spTree></p:cSld></p:sld>'
    )
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, 'w') as zf:
        zf.writestr('ppt/presentation.xml', presentation_xml)
        zf.writestr('ppt/_rels/presentation.xml.rels', rels_xml)
        zf.writestr('ppt/slides/slide1.xml', slide_xml)
    text = extract_chat_document(request('deck.pptx', buffer.getvalue()))['data']['content']
    assert text == '[Slide 1]\nTitle'
