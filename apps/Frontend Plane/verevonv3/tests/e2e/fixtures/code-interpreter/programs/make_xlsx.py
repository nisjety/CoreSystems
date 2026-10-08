# Reference program for the H5 "xlsx" case: what a good code_interpreter call
# writes for the prompt in tests/e2e/code-interpreter-proof.ts. It is run by
# h5_oracle.py (reference, self-test) and by the live-stack check in the runbook,
# never by the model. Invented numbers only.
from openpyxl import Workbook
from openpyxl.chart import BarChart, Reference

wb = Workbook()
ws = wb.active
ws.title = "Salg Q3"
ws.append(["Måned", "Omsetning", "Kostnad", "Margin"])
rows = [("Juli", 120000, 80000), ("August", 135000, 91000), ("September", 150000, 98000)]
for index, (month, revenue, cost) in enumerate(rows, start=2):
    ws.append([month, revenue, cost, f"=B{index}-C{index}"])
ws.append(["Sum", "=SUM(B2:B4)", "=SUM(C2:C4)", "=SUM(D2:D4)"])

chart = BarChart()
chart.type = "col"
chart.title = "Omsetning per måned"
chart.y_axis.title = "NOK"
chart.add_data(Reference(ws, min_col=2, min_row=1, max_row=4), titles_from_data=True)
chart.set_categories(Reference(ws, min_col=1, min_row=2, max_row=4))
ws.add_chart(chart, "F2")

wb.save("salg-q3.xlsx")
print("saved salg-q3.xlsx")
