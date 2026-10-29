---
name: PDF (pdf)
description: Build a PDF with reportlab in code_interpreter, read or merge an existing PDF with pypdf, and check the result before delivery.
---

# PDF documents (.pdf)

Use this for a printable, fixed-layout document, or whenever the user asks for a PDF.

## Build

- Create new PDFs with reportlab's platypus layer (SimpleDocTemplate, Paragraph, Table, Spacer, Image, PageBreak) and its sample styles. The built-in Helvetica covers Latin text, including the Norwegian letters æ, ø and å.
- For other scripts, register a TrueType font first. The sandbox has DejaVu and Liberation fonts under /usr/share/fonts/truetype.
- Write the document in the language the user wrote in. Keep figures exactly as given, and state any assumption on the page.
- Tables: wrap cell text in Paragraphs so long text wraps, and repeat the header row on each page of a long table.
- Charts: a matplotlib PNG (plt.savefig, never plt.show) placed with Image, sized to the frame width.
- To read or merge a PDF the user supplied, use pypdf (PdfReader, PdfWriter). Do not rebuild a file the user already has.

## Deliver

- Save the final file at the top level of the working directory, for example "tilbud.pdf".
- Reopen it with pypdf and print the page count and the first page's text, to confirm it reads back.
- When the code_interpreter result has an office_checks line for a file, that line is the file's structural check. A file that says FAILED, or that has zero pages, must be regenerated before you call it ready. Describe a file as structurally checked only when such a line says so, and never as verified in a PDF reader.
