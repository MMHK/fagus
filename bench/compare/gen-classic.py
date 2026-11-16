# Generates a classic invoice (CSS 2.1 only: tables, no flex, grid or margin boxes) with N lines,
# so that every engine in the comparison can lay it out.
import sys
n = int(sys.argv[1])
rows = "\n".join(
    f'<tr><td class="pos">{i}</td><td><strong>Webentwicklung Sprint {i}</strong><br><span class="muted">Kundenportal, Umsetzung und Tests</span></td>'
    f'<td class="num">2 Std.</td><td class="num">95,00 &euro;</td><td class="num">190,00 &euro;</td></tr>'
    for i in range(1, n + 1))
net = 190 * n
print(f"""<!doctype html>
<html lang="de"><head><meta charset="utf-8"><title>Rechnung RE-2026-0042</title>
<style>
@font-face {{ font-family: Inter; src: url(fonts/Inter-Regular.ttf); font-weight: normal; }}
@font-face {{ font-family: Inter; src: url(fonts/Inter-Bold.ttf); font-weight: bold; }}
@page {{ size: 210mm 297mm; margin: 20mm 18mm 25mm 20mm; }}
body {{ margin: 0; font-family: Inter, sans-serif; font-size: 9.5pt; line-height: 1.45; color: #111827; }}
table {{ width: 100%; border-collapse: collapse; }}
.head td {{ vertical-align: top; padding: 0; }}
.head {{ border-bottom: 2px solid #2741d9; margin-bottom: 10mm; }}
.company {{ font-size: 15pt; font-weight: bold; color: #2741d9; }}
.muted {{ color: #6b7280; font-size: 8pt; }}
.sender {{ font-size: 7pt; color: #6b7280; border-bottom: 1px solid #e5e7eb; padding-bottom: 1mm; margin-bottom: 2mm; }}
.meta td {{ padding: 0.5mm 0; font-size: 8.5pt; }}
.meta td.v {{ text-align: right; font-weight: bold; }}
h1 {{ font-size: 18pt; margin: 12mm 0 2mm; }}
.items th {{ background: #f3f4f6; text-align: left; font-size: 8pt; text-transform: uppercase; color: #374151; padding: 2mm; }}
.items td {{ padding: 2.2mm 2mm; border-bottom: 1px solid #e5e7eb; vertical-align: top; }}
.items tr {{ page-break-inside: avoid; }}
.num {{ text-align: right; white-space: nowrap; }}
.pos {{ color: #6b7280; width: 8mm; }}
.totals {{ width: 75mm; margin: 5mm 0 0 auto; }}
.totals td {{ padding: 1mm 0; }}
.totals td.v {{ text-align: right; }}
.totals tr.grand td {{ border-top: 2px solid #111827; font-weight: bold; font-size: 11pt; padding-top: 2mm; }}
.note {{ margin-top: 10mm; padding: 4mm; border-left: 3px solid #2741d9; background: #f9fafb; page-break-inside: avoid; }}
</style></head>
<body>
<table class="head"><tr><td><div class="company">Muster GmbH</div><div class="muted">Webentwicklung und Beratung</div><br></td><td style="text-align:right" class="muted">Hauptstra&szlig;e 1<br>12345 Musterstadt</td></tr></table>
<table><tr><td style="vertical-align:top;width:55%"><div class="sender">Muster GmbH &middot; Hauptstra&szlig;e 1 &middot; 12345 Musterstadt</div>Kunde AG<br>z. Hd. Erika Beispiel<br>Bahnhofstra&szlig;e 5<br>54321 Beispielstadt</td>
<td style="vertical-align:top"><table class="meta"><tr><td>Rechnungsnummer</td><td class="v">RE-2026-0042</td></tr><tr><td>Rechnungsdatum</td><td class="v">01.11.2026</td></tr><tr><td>Leistungsdatum</td><td class="v">30.09.2026</td></tr><tr><td>Kundennummer</td><td class="v">K-1007</td></tr></table></td></tr></table>
<h1>Rechnung</h1>
<p>Vielen Dank f&uuml;r Ihren Auftrag. Wir berechnen Ihnen folgende Leistungen:</p>
<table class="items">
  <thead><tr><th>Pos.</th><th>Beschreibung</th><th class="num">Menge</th><th class="num">Einzelpreis</th><th class="num">Gesamt</th></tr></thead>
  <tbody>
{rows}
  </tbody>
</table>
<table class="totals">
  <tr><td>Nettobetrag</td><td class="v">{net:,.2f} &euro;</td></tr>
  <tr><td>Umsatzsteuer 19 %</td><td class="v">{net*0.19:,.2f} &euro;</td></tr>
  <tr class="grand"><td>Gesamtbetrag</td><td class="v">{net*1.19:,.2f} &euro;</td></tr>
</table>
<div class="note">Bitte &uuml;berweisen Sie den Betrag bis zum 15.11.2026 auf das Konto DE02 1203 0000 0000 2020 51.</div>
</body></html>""")
