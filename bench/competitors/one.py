import sys, io
FONTS = ["/bench/fonts/Inter-Regular.ttf", "/bench/fonts/Inter-Bold.ttf"]
which = sys.argv[1]; html = open("/bench/modern.html", encoding="utf-8").read()
if which == "pluto":
    import plutoprint as m
    book = m.Book(m.PAGE_SIZE_A4); book.load_html(html, base_url="file:///bench/"); buf = io.BytesIO(); book.write_to_pdf_stream(buf); pdf = buf.getvalue()
elif which == "fulgur":
    import pyfulgur as m
    b = m.AssetBundle()
    for f in FONTS: b.add_font_file(f)
    pdf = m.Engine.builder().page_size(m.PageSize.A4).assets(b).build().render_html(html)
elif which == "fullbleed":
    import fullbleed as m
    pdf = m.PdfEngine(font_files=FONTS).render_pdf(html, "")
elif which == "ironpress":
    import ironpress as m
    c = m.HtmlConverter(); c.page_size("A4"); c.base_path("/bench"); pdf = c.convert(html)
open(f"/bench/out-{which}.pdf", "wb").write(pdf); print(which, len(pdf))
