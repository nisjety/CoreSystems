---
name: Excel (xlsx)
description: Build an Excel .xlsx workbook with openpyxl or xlsxwriter in code_interpreter: live formulas, labelled inputs, native charts, and the checks before delivery.
---

# Excel workbooks (.xlsx)

Use this for a budget, schedule, tracker, calculation or any spreadsheet the user will open in Excel.

## Build

- Put inputs and assumptions in labelled cells, and refer to them from formulas. Do not bury constants inside formulas.
- Write formulas as strings, for example "=SUM(B2:B9)", so the workbook keeps live formulas. Neither openpyxl nor xlsxwriter calculates them. Compute each figure you quote in Python as well, and say which values are formulas.
- One header row per table, frozen with freeze_panes. Set number formats (for example '#,##0.00' or 'yyyy-mm-dd') and column widths. No merged cells inside data tables.
- Charts: use a native chart (openpyxl.chart or xlsxwriter add_chart) that points at the sheet cells.
- No macros, external links or data connections. Save as .xlsx, never .xlsm.

## Deliver

- Save the final file at the top level of the working directory, for example "budsjett.xlsx".
- Reopen it with openpyxl (load_workbook) and print the sheet names and the first rows, to confirm it reads back.
- When the code_interpreter result has an office_checks line for a file, that line is the file's structural check. If it says FAILED, or a cell holds a literal error value such as #DIV/0!, fix it and regenerate the file before you call it ready. Describe a file as structurally checked only when such a line says so, and never as verified in Excel.
