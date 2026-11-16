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
elif which == "fulgur":
    import pyfulgur as m
    b = m.AssetBundle()
    for f in FONTS: b.add_font_file(f)
    eng = m.Engine.builder().page_size(m.PageSize.A4).assets(b).build()
    render = lambda e, h: e.render_html(h)

base = open("/bench/n0.html", encoding="utf-8").read()
def add_css(h, css): return h.replace("</style>", css + "\n</style>")
variants = {
    "full (n=0)": base,
    "no bold font": add_css(base, "* { font-weight: 400 !important; }").replace('@font-face { font-family: Inter; src: url(fonts/Inter-Bold.ttf); font-weight: 700; }', ''),
    "no margin boxes": re.sub(r"@bottom-(left|right) \{[^}]*\}", "", base),
    "no flex/grid": add_css(base, "* { display: block !important; }"),
    "no CSS at all": re.sub(r"<style>.*?</style>", "", base, flags=re.S),
    "text only": "<!doctype html><html><body><p>" + re.sub(r"<[^>]+>", " ", re.sub(r"<style>.*?</style>", "", base, flags=re.S)) + "</p></body></html>",
}
render(eng, base)
for name, h in variants.items():
    t = []
    for _ in range(9):
        s = time.perf_counter(); pdf = render(eng, h); t.append((time.perf_counter() - s) * 1000)
    print(f"{which:9s} {name:16s} {statistics.median(t):7.1f} ms  pdf={len(pdf)//1024}KB", flush=True)
