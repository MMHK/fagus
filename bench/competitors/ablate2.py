import sys, time, statistics, io, re
FONTS = ["/bench/fonts/Inter-Regular.ttf", "/bench/fonts/Inter-Bold.ttf"]
which = sys.argv[1]
if which == "pluto":
    import plutoprint as m
    eng = None
    def render(_, h):
        book = m.Book(m.PAGE_SIZE_A4); book.load_html(h, base_url="file:///bench/")
        buf = io.BytesIO(); book.write_to_pdf_stream(buf); return buf.getvalue()
elif which == "fullbleed":
    import fullbleed as m
    eng = m.PdfEngine(font_files=FONTS)
    render = lambda e, h: e.render_pdf(h, "")
base = open("/bench/n0.html", encoding="utf-8").read()
no_fontface = re.sub(r"@font-face \{[^}]*\}", "", base)
no_page = re.sub(r"@page \{.*?\} \}", "", base, flags=re.S)
variants = {
    "full (n=0)": base,
    "no @font-face": no_fontface,
    "no @page": no_page,
    "no @font-face, no @page": re.sub(r"@page \{.*?\} \}", "", no_fontface, flags=re.S),
    "no CSS, font via body": re.sub(r"<style>.*?</style>", "<style>body{font-family:Inter}</style>", base, flags=re.S),
}
render(eng, base)
for name, h in variants.items():
    t = []
    for _ in range(9):
        s = time.perf_counter(); pdf = render(eng, h); t.append((time.perf_counter() - s) * 1000)
    print(f"{which:9s} {name:26s} {statistics.median(t):7.1f} ms  pdf={len(pdf)//1024}KB", flush=True)
