//! Parses an arbitrary stylesheet, as a sheet and as the declarations of a `style` attribute,
//! and computes the styles of a fixed document with it.
#![no_main]

use std::sync::{Arc, OnceLock};

use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
use libfuzzer_sys::fuzz_target;

/// Elements of many kinds for the selectors and properties of the stylesheet to meet.
const DOCUMENT: &str = r##"<!DOCTYPE html>
<html lang="de"><head><title>T</title></head>
<body class="a b" id="top" dir="ltr">
<h1 class="title">Rechnung <small>Nr. 1</small></h1>
<p id="p1" class="lead" style="color: red">Text <a href="#top">Link</a> <em>em</em> <b>b</b>
<span lang="ar" dir="rtl">مرحبا</span> <code>code</code> <sup>1</sup></p>
<ul><li>eins</li><li class="x">zwei<ol start="3"><li>drei</li></ol></li></ul>
<table><caption>c</caption><thead><tr><th colspan="2">h</th></tr></thead>
<tbody><tr><td>1</td><td rowspan="2">2</td></tr><tr><td>3</td></tr></tbody></table>
<div class="grid"><div>a</div><div>b</div><section><article><p>c</p></article></section></div>
<form><input type="text" value="v"><input type="checkbox" checked><select><option>o</option>
</select><textarea>t</textarea><button>b</button><progress value="1" max="2"></progress></form>
<img src="a.png" alt="a"><svg width="10" height="10"><rect width="5" height="5"/></svg>
<details open><summary>s</summary>d</details><blockquote>q</blockquote><hr><pre>pre</pre>
<math><mfrac><mi>a</mi><mn>2</mn></mfrac></math>
</body></html>"##;

fn user_agent() -> &'static Arc<Stylesheet> {
    static SHEET: OnceLock<Arc<Stylesheet>> = OnceLock::new();
    SHEET.get_or_init(|| {
        Arc::new(Stylesheet::parse(
            fagus_style::USER_AGENT_CSS,
            Origin::UserAgent,
            "fagus:ua.css",
        ))
    })
}

fuzz_target!(|data: &[u8]| {
    let css = fagus_fuzz::text(data);
    let _ = fagus_css::parse_declarations(&css);
    let _ = fagus_css::style_attribute_diagnostics(&css, "style", 1);
    let sheet = Stylesheet::parse(&css, Origin::Author, "fuzz.css");
    let _ = sheet.diagnostics();
    let document = fagus_dom::Document::parse_html(DOCUMENT);
    let rules = RuleSet::new(&[user_agent(), &sheet], &MediaEnvironment::default());
    let styled = fagus_style::compute_styles(&document, &rules, (794.0, 1123.0));
    let _ = styled.all().count();
});
