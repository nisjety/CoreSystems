import base64
import io
import pytest
from docx import Document
from fastapi import HTTPException
from app.chat_extract import ChatExtractRequest, extract_chat_document


def request(name, content):
    return ChatExtractRequest(filename=name, content_base64=base64.b64encode(content).decode())


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


@pytest.mark.parametrize('name,content,status', [
    ('broken.pdf', b'not a PDF', 422), ('empty.md', b' ', 422),
    ('large.md', b'x' * 60_001, 413), ('unknown.exe', b'binary', 415),
])
def test_unreadable_or_oversized_input_is_rejected(name, content, status):
    with pytest.raises(HTTPException) as exc:
        extract_chat_document(request(name, content))
    assert exc.value.status_code == status
