import sys, time, statistics, importlib, io

def ms(t): return t * 1000
FONTS = ["/bench/fonts/Inter-Regular.ttf", "/bench/fonts/Inter-Bold.ttf"]

def bench(name, make, render, html, runs):
    t0 = time.perf_counter(); eng = make(); t_make = time.perf_counter() - t0
    t0 = time.perf_counter(); out = render(eng, html); t_first = time.perf_counter() - t0
    warm = []
    for _ in range(runs):
        t0 = time.perf_counter(); out = render(eng, html); warm.append(time.perf_counter() - t0)
    fresh = []
    for _ in range(max(3, runs // 3)):
        t0 = time.perf_counter(); e = make(); out2 = render(e, html); fresh.append(time.perf_counter() - t0)
    print(f"{name:10s} first(make+render)={ms(t_make + t_first):8.2f}ms warm_median={ms(statistics.median(warm)):8.2f}ms new_engine_each={ms(statistics.median(fresh)):8.2f}ms pdf={len(out)//1024}KB", flush=True)
    return out

which, file, runs = sys.argv[1], sys.argv[2], int(sys.argv[3])
html = open(file, encoding="utf-8").read()
t0 = time.perf_counter()
if which == "fulgur":
    m = importlib.import_module("pyfulgur"); t_imp = time.perf_counter() - t0
    def make():
        b = m.AssetBundle()
        for f in FONTS: b.add_font_file(f)
        return m.Engine.builder().page_size(m.PageSize.A4).assets(b).build()
    out = bench("fulgur", make, lambda e, h: e.render_html(h), html, runs)
elif which == "ironpress":
    m = importlib.import_module("ironpress"); t_imp = time.perf_counter() - t0
    def make():
        c = m.HtmlConverter(); c.page_size("A4"); c.base_path("/bench"); return c
    out = bench("ironpress", make, lambda c, h: c.convert(h), html, runs)
elif which == "fullbleed":
    m = importlib.import_module("fullbleed"); t_imp = time.perf_counter() - t0
    out = bench("fullbleed", lambda: m.PdfEngine(font_files=FONTS), lambda e, h: e.render_pdf(h, ""), html, runs)
elif which == "pluto":
    m = importlib.import_module("plutoprint"); t_imp = time.perf_counter() - t0
    def render(_, h):
        book = m.Book(m.PAGE_SIZE_A4); book.load_html(h, base_url="file:///bench/")
        buf = io.BytesIO(); book.write_to_pdf_stream(buf); return buf.getvalue()
    out = bench("plutobook", lambda: None, render, html, runs)
print(f"{'':10s} import={ms(t_imp):.1f}ms", flush=True)
open(f"/bench/out-{which}-{file.split('/')[-1].replace('.html','')}.pdf", "wb").write(out)
