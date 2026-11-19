//! Which installed families Chrome takes for the generic families and for the standard font,
//! by the language of the text, as Chrome on macOS sets them (its default font settings per
//! script), with the families of Windows and Linux after them for systems without the Mac's.

/// A generic family of CSS, or the standard font: what text without `font-family` is set in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Generic {
    Standard,
    Serif,
    SansSerif,
    Monospace,
    Cursive,
    Fantasy,
    SystemUi,
    /// `math`: a font with a `MATH` table for `MathML`.
    Math,
}

impl Generic {
    /// The generic family a `font-family` name stands for, in any case.
    pub(crate) fn of(family: &str) -> Option<Self> {
        Some(match family.to_ascii_lowercase().as_str() {
            "-webkit-standard" => Self::Standard,
            "serif" | "ui-serif" => Self::Serif,
            "sans-serif" | "ui-sans-serif" | "ui-rounded" => Self::SansSerif,
            "monospace" | "ui-monospace" => Self::Monospace,
            "cursive" => Self::Cursive,
            "fantasy" => Self::Fantasy,
            "system-ui" | "-apple-system" | "blinkmacsystemfont" => Self::SystemUi,
            "math" => Self::Math,
            _ => return None,
        })
    }
}

/// The scripts for which Chrome has fonts of their own (its font settings per script).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LangScript {
    Japanese,
    Korean,
    SimplifiedChinese,
    TraditionalChinese,
    Devanagari,
}

impl LangScript {
    /// The script of a BCP 47 language tag, as Chrome picks the font settings for it: Chinese
    /// is simplified unless the tag says `Hant` or a region that writes traditional characters
    /// (Taiwan, Hong Kong, Macao). A script subtag decides over the language, as in Chrome's
    /// `LayoutLocale`: Chinese in Latin letters (`zh-Latn-pinyin`) takes no font of its own.
    pub(crate) fn of(lang: &str) -> Option<Self> {
        let mut subtags = lang.split(['-', '_']).map(str::to_ascii_lowercase);
        let language = subtags.next()?;
        let script = lang
            .split(['-', '_'])
            .skip(1)
            .find(|subtag| subtag.len() == 4 && subtag.bytes().all(|byte| byte.is_ascii_alphabetic()))
            .map(str::to_ascii_lowercase);
        if let Some(script) = script {
            return match script.as_str() {
                "jpan" | "hira" | "kana" | "hrkt" => Some(Self::Japanese),
                "kore" | "hang" => Some(Self::Korean),
                "hans" => Some(Self::SimplifiedChinese),
                "hant" => Some(Self::TraditionalChinese),
                "hani" => Self::of(&language),
                "deva" => Some(Self::Devanagari),
                _ => None,
            };
        }
        Some(match language.as_str() {
            "ja" => Self::Japanese,
            "ko" => Self::Korean,
            "zh" => {
                let traditional =
                    subtags.any(|subtag| matches!(subtag.as_str(), "hant" | "tw" | "hk" | "mo"));
                if traditional {
                    Self::TraditionalChinese
                } else {
                    Self::SimplifiedChinese
                }
            }
            "hi" | "mr" | "ne" => Self::Devanagari,
            _ => return None,
        })
    }
}

/// The families whose glyphs have the same widths as those of `family`, which fontconfig puts in
/// its place on Linux where it is not installed and which Chrome accepts there (Skia's
/// `kFontEquivMap`): the free Liberation, Croscore and Caladea/Carlito families for Microsoft's
/// and Adobe's core fonts. Empty for other families.
pub(crate) fn metric_equivalents(family: &str) -> &'static [&'static str] {
    const CLASSES: [&[&str]; 6] = [
        &[
            "Arial",
            "Helvetica",
            "Liberation Sans",
            "Arimo",
            "Albany AMT",
            "Albany",
        ],
        &[
            "Times New Roman",
            "Times",
            "Liberation Serif",
            "Tinos",
            "Thorndale AMT",
            "Thorndale",
        ],
        &[
            "Courier New",
            "Courier",
            "Liberation Mono",
            "Cousine",
            "Cumberland AMT",
            "Cumberland",
        ],
        &["Cambria", "Caladea"],
        &["Calibri", "Carlito"],
        &["Symbol", "Symbol Neu"],
    ];
    CLASSES
        .into_iter()
        .find(|class| class.iter().any(|member| member.eq_ignore_ascii_case(family)))
        .unwrap_or(&[])
}

/// The families that stand for a generic family in text of a language, the first one installed
/// first: the family of the language's script where Chrome has one, then the default.
pub(crate) fn generic_families(generic: Generic, script: Option<LangScript>) -> Vec<&'static str> {
    let own = script_families(generic, script);
    let default: &[&str] = match generic {
        Generic::Standard | Generic::Serif => &[
            "Times",
            "Times New Roman",
            "Liberation Serif",
            "DejaVu Serif",
            "Noto Serif",
        ],
        Generic::SansSerif => &[
            "Helvetica",
            "Arial",
            "Liberation Sans",
            "DejaVu Sans",
            "Noto Sans",
        ],
        Generic::Monospace => &[
            "Menlo",
            "Consolas",
            "DejaVu Sans Mono",
            "Liberation Mono",
            "Noto Sans Mono",
            "Courier New",
        ],
        Generic::Cursive => &["Apple Chancery", "Comic Sans MS", "URW Chancery L"],
        Generic::Fantasy => &["Papyrus", "Impact"],
        // The system font of macOS (San Francisco), hidden from documents, and of Windows.
        Generic::SystemUi => &[".SF NS", "System Font", "Segoe UI"],
        // Chrome's math font on macOS, then those of Windows and Linux.
        Generic::Math => &[
            "STIX Two Math",
            "Cambria Math",
            "Latin Modern Math",
            "STIX Math",
            "Libertinus Math",
            "TeX Gyre Termes Math",
            "Noto Sans Math",
        ],
    };
    own.iter().chain(default).copied().collect()
}

/// The families fallback tries first for a character of text in a language of `script`, before
/// the families of `sans-serif`: fontconfig on Linux ranks the fonts that support the text's
/// language first, so Chinese, Japanese and Korean text takes Noto Sans CJK in its own variant.
pub(crate) fn fallback_families(script: Option<LangScript>) -> &'static [&'static str] {
    match script {
        Some(LangScript::Japanese) => &["Noto Sans CJK JP"],
        Some(LangScript::Korean) => &["Noto Sans CJK KR"],
        Some(LangScript::SimplifiedChinese) => &["Noto Sans CJK SC"],
        Some(LangScript::TraditionalChinese) => &["Noto Sans CJK TC"],
        Some(LangScript::Devanagari) | None => &[],
    }
}

/// The families that Chrome takes for a generic family in text of a language of `script`, before
/// the default ones: none for languages without their own.
pub(crate) fn script_families(generic: Generic, script: Option<LangScript>) -> &'static [&'static str] {
    use LangScript::{Devanagari, Japanese, Korean, SimplifiedChinese, TraditionalChinese};
    match (generic, script) {
        // Chrome on Linux has no fonts of its own for these languages: their text takes the
        // default families, and fallback finds the characters (see [`fallback_families`]).
        (Generic::Standard | Generic::SansSerif, Some(Japanese)) => {
            &["Hiragino Kaku Gothic ProN", "Yu Gothic", "Meiryo"]
        }
        (Generic::Serif, Some(Japanese)) => &["Hiragino Mincho ProN", "Yu Mincho"],
        (Generic::Standard | Generic::SansSerif, Some(Korean)) => &["Apple SD Gothic Neo", "Malgun Gothic"],
        (Generic::Serif, Some(Korean)) => &["AppleMyungjo", "Batang"],
        (Generic::Standard | Generic::SansSerif | Generic::Cursive, Some(SimplifiedChinese)) => {
            &["PingFang SC", "Microsoft YaHei"]
        }
        (Generic::Serif, Some(SimplifiedChinese)) => &["Songti SC", "SimSun"],
        (Generic::Standard | Generic::SansSerif | Generic::Cursive, Some(TraditionalChinese)) => {
            &["PingFang TC", "Microsoft JhengHei"]
        }
        (Generic::Serif, Some(TraditionalChinese)) => &["Songti TC", "PMingLiU"],
        // Chrome's Devanagari MT has no Latin letters: they come from Times New Roman, spaces,
        // digits and punctuation from it.
        (Generic::Standard | Generic::Serif, Some(Devanagari)) => &["Devanagari MT", "Times New Roman"],
        (Generic::SansSerif, Some(Devanagari)) => &["Kohinoor Devanagari", "Nirmala UI"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_script_of_a_language() {
        assert_eq!(LangScript::of("ja"), Some(LangScript::Japanese));
        assert_eq!(LangScript::of("ja-JP"), Some(LangScript::Japanese));
        assert_eq!(LangScript::of("ko-KR"), Some(LangScript::Korean));
        assert_eq!(LangScript::of("zh"), Some(LangScript::SimplifiedChinese));
        // A script subtag decides.
        assert_eq!(LangScript::of("zh-Latn-pinyin"), None);
        assert_eq!(LangScript::of("ja-Latn"), None);
        assert_eq!(LangScript::of("zh-Hant"), Some(LangScript::TraditionalChinese));
        assert_eq!(LangScript::of("sr-Hans"), Some(LangScript::SimplifiedChinese));
        assert_eq!(LangScript::of("zh-Hans-CN"), Some(LangScript::SimplifiedChinese));
        assert_eq!(LangScript::of("zh-SG"), Some(LangScript::SimplifiedChinese));
        assert_eq!(LangScript::of("zh-Hant"), Some(LangScript::TraditionalChinese));
        assert_eq!(LangScript::of("zh-TW"), Some(LangScript::TraditionalChinese));
        assert_eq!(LangScript::of("zh_HK"), Some(LangScript::TraditionalChinese));
        assert_eq!(LangScript::of("en"), None);
        assert_eq!(LangScript::of(""), None);
    }

    #[test]
    fn lists_the_families_of_generic_families() {
        let japanese = Some(LangScript::Japanese);
        assert_eq!(generic_families(Generic::Serif, None)[0], "Times");
        assert_eq!(generic_families(Generic::SansSerif, None)[0], "Helvetica");
        assert_eq!(
            generic_families(Generic::Serif, japanese)[0],
            "Hiragino Mincho ProN"
        );
        assert_eq!(
            generic_families(Generic::Standard, japanese)[0],
            "Hiragino Kaku Gothic ProN"
        );
        // Monospace stays Menlo in Japanese text, as Chrome's Osaka-Mono is not installed.
        assert_eq!(generic_families(Generic::Monospace, japanese)[0], "Menlo");
        assert_eq!(
            generic_families(Generic::Cursive, Some(LangScript::TraditionalChinese))[0],
            "PingFang TC"
        );
        assert_eq!(
            generic_families(Generic::Standard, Some(LangScript::Devanagari))[..2],
            ["Devanagari MT", "Times New Roman"]
        );
        assert_eq!(Generic::of("BlinkMacSystemFont"), Some(Generic::SystemUi));
        assert_eq!(Generic::of("Inter"), None);
    }
}
