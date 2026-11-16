import sys, time, statistics, io, subprocess

FONTS = ["/bench/fonts/Inter-Regular.ttf", "/bench/fonts/Inter-Bold.ttf"]
which = sys.argv[1]
sizes = [0, 1, 10, 30, 60, 150, 300]

if which == "fulgur":
    import pyfulgur as m
    def make():
        b = m.AssetBundle()
        for f in FONTS: b.add_font_file(f)
        return m.Engine.builder().page_size(m.PageSize.A4).assets(b).build()
    render = lambda e, h: e.render_html(h)
elif which == "ironpress":
    import ironpress as m
    def make():
        c = m.HtmlConverter(); c.page_size("A4"); c.base_path("/bench"); return c
    render = lambda c, h: c.convert(h)
elif which == "fullbleed":
    import fullbleed as m
    make = lambda: m.PdfEngine(font_files=FONTS)
    render = lambda e, h: e.render_pdf(h, "")
elif which == "pluto":
    import plutoprint as m
    make = lambda: None
    def render(_, h):
        book = m.Book(m.PAGE_SIZE_A4); book.load_html(h, base_url="file:///bench/")
        buf = io.BytesIO(); book.write_to_pdf_stream(buf); return buf.getvalue()

eng = make()
render(eng, open("/bench/n1.html", encoding="utf-8").read())  # warm-up
for n in sizes:
    html = open(f"/bench/n{n}.html", encoding="utf-8").read()
    runs = 7 if n <= 30 else 3
    t = []
    for _ in range(runs):
        s = time.perf_counter(); pdf = render(eng, html); t.append((time.perf_counter() - s) * 1000)
    open("/tmp/o.pdf", "wb").write(pdf)
    pages = next((l.split()[-1] for l in subprocess.run(["pdfinfo", "/tmp/o.pdf"], capture_output=True, text=True).stdout.splitlines() if l.startswith("Pages")), "?")
    print(f"{which:9s} n={n:3d} pages={pages:>2s} median={statistics.median(t):8.1f} ms", flush=True)
