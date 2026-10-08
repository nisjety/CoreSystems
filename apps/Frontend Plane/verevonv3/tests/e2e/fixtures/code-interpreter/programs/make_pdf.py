# Reference program for the H5 "pdf" case. See programs/make_xlsx.py.
# reportlab's built-in Helvetica is WinAnsi encoded, which covers æ, ø and å.
from reportlab.lib.pagesizes import A4
from reportlab.pdfgen import canvas

pdf = canvas.Canvas("sammendrag.pdf", pagesize=A4)
width, height = A4
pdf.setFont("Helvetica-Bold", 20)
pdf.drawString(72, height - 96, "Sammendrag")
pdf.setFont("Helvetica", 12)
pdf.drawString(72, height - 130, "Rød grøt med fløte er søt og sunn, og blåbær fra Ålesund smaker best.")
pdf.save()
print("saved sammendrag.pdf")
