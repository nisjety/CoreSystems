---
name: Word (docx)
description: Build a Word .docx file with python-docx in code_interpreter: headings and styles, tables, images, and the checks to run before delivery.
---

# Word documents (.docx)

Use this for a letter, memo, report, minutes, draft or any document the user will open in Word.

## Build

- Write the file with python-docx. Use built-in styles ("Title", "Heading 1", "Heading 2", "List Bullet", "Table Grid") instead of bold text standing in for headings.
- Write the document in the language the user wrote in. Keep names, figures and dates exactly as given. If you had to assume a value, say so in the text.
- Tables: one bold header row, one row per record, numbers right-aligned.
- Charts or diagrams: draw a PNG with matplotlib (plt.savefig, never plt.show) and add it with add_picture, at most 16 cm wide.
- Do not use fields that need updating on open, such as a table of contents. Write the section list as plain text.

## Deliver

- Save the final file at the top level of the working directory, for example "rapport.docx". Files in subfolders are not returned.
- Reopen the file with python-docx and print its headings and table count, to confirm it reads back.
- When the code_interpreter result has an office_checks line for a file, that line is the file's structural check. If it says FAILED, regenerate the file, fix the reported problem and run it again before you call it ready. Describe a file as structurally checked only when such a line says so, and never as verified in Word.
