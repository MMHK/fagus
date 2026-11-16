//! The operator dictionary of `MathML` Core (§B), as Blink stores it compactly
//! (`mathml_operator_dictionary.cc`, BSD licensed: see `CHROMIUM-LICENSE`), the properties of
//! its categories, and the characters that stretch along the inline axis.

/// The form of an operator: before, between or after its operands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Form {
    Infix,
    Prefix,
    Postfix,
}

/// The properties an operator takes from the dictionary: its spacing in eighteenths of an em
/// and its flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the flags of MathML Core's operators"
)]
pub(crate) struct Entry {
    pub lspace: u8,
    pub rspace: u8,
    pub stretchy: bool,
    pub symmetric: bool,
    pub largeop: bool,
    pub movablelimits: bool,
}

impl Entry {
    const fn new(lspace: u8, rspace: u8, flags: u8) -> Self {
        Self {
            lspace,
            rspace,
            stretchy: flags & STRETCHY != 0,
            symmetric: flags & SYMMETRIC != 0,
            largeop: flags & LARGEOP != 0,
            movablelimits: flags & MOVABLELIMITS != 0,
        }
    }

    /// The default properties of an operator the dictionary does not list.
    pub(crate) const DEFAULT: Self = Self::new(5, 5, 0);
}

const STRETCHY: u8 = 1;
const SYMMETRIC: u8 = 2;
const LARGEOP: u8 = 4;
const MOVABLELIMITS: u8 = 8;

/// The categories of `MathML` Core's operator dictionary that Blink distinguishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    ForceDefault,
    A,
    B,
    C,
    Dek,
    FG,
    H,
    I,
    J,
    L,
    M,
}

impl Category {
    const fn entry(self) -> Entry {
        match self {
            Self::ForceDefault => Entry::DEFAULT,
            Self::A => Entry::new(5, 5, STRETCHY),
            Self::B => Entry::new(4, 4, 0),
            Self::C => Entry::new(3, 3, 0),
            Self::Dek => Entry::new(0, 0, 0),
            Self::FG => Entry::new(0, 0, STRETCHY | SYMMETRIC),
            Self::H => Entry::new(3, 3, SYMMETRIC | LARGEOP),
            Self::I => Entry::new(0, 0, STRETCHY),
            Self::J => Entry::new(3, 3, SYMMETRIC | LARGEOP | MOVABLELIMITS),
            Self::L => Entry::new(3, 0, 0),
            Self::M => Entry::new(0, 3, 0),
        }
    }
}

/// Operators of two ASCII characters, mapped to keys from U+0320 on in this order.
const TWO_ASCII: [&str; 18] = [
    "!!", "!=", "&&", "**", "*=", "++", "+=", "--", "-=", "->", "//", "/=", ":=", "<=", "<>", "==", ">=",
    "||",
];

/// The compact dictionary: a key (with the form in bits 12 and 13 and the category in the top
/// four bits) and how many keys after it the entry covers.
const DICTIONARY: &[(u16, u8)] = &[
    (0x8025, 0),
    (0x802A, 0),
    (0x402B, 0),
    (0x402D, 0),
    (0x802E, 0),
    (0x402F, 0),
    (0x803F, 1),
    (0xC05C, 0),
    (0x805E, 0),
    (0xC05F, 0),
    (0x40B1, 0),
    (0x80B7, 0),
    (0x80D7, 0),
    (0x40F7, 0),
    (0x4322, 0),
    (0x8323, 0),
    (0x832E, 0),
    (0x8422, 0),
    (0x8443, 0),
    (0x4444, 0),
    (0xC461, 3),
    (0x0590, 5),
    (0x059A, 15),
    (0x05AA, 4),
    (0x05B0, 5),
    (0x05B9, 0),
    (0x05BC, 15),
    (0x05CC, 9),
    (0x05DA, 15),
    (0x05EA, 6),
    (0x05F3, 12),
    (0xC606, 0),
    (0x4612, 4),
    (0x8617, 2),
    (0x4627, 3),
    (0x4636, 0),
    (0x4638, 0),
    (0x8640, 0),
    (0x468C, 2),
    (0x4693, 3),
    (0x8697, 0),
    (0x4698, 0),
    (0x8699, 2),
    (0x469D, 2),
    (0x86A0, 1),
    (0x86BA, 0),
    (0x46BB, 2),
    (0x86C4, 3),
    (0x86C9, 3),
    (0x46CE, 1),
    (0x46D2, 1),
    (0x8705, 1),
    (0x0B94, 0),
    (0x4B95, 2),
    (0x0B99, 0),
    (0x0B9B, 6),
    (0x0BA5, 1),
    (0x0BA8, 7),
    (0x0BB1, 0),
    (0x0BB3, 0),
    (0x0BB5, 0),
    (0x0BB8, 0),
    (0x0BBA, 4),
    (0x8BCB, 0),
    (0x8BCD, 0),
    (0x0BF0, 1),
    (0x0BF4, 11),
    (0x0D00, 15),
    (0x0D10, 15),
    (0x0D20, 0),
    (0x0D34, 3),
    (0x0D42, 15),
    (0x0D52, 15),
    (0x0D62, 15),
    (0x0D72, 3),
    (0x0D7C, 3),
    (0x4DB8, 0),
    (0x4DBC, 0),
    (0x4DC4, 1),
    (0x8DC6, 2),
    (0x8DD4, 3),
    (0x8DE2, 0),
    (0x4DF5, 6),
    (0x8E1D, 1),
    (0x4E1F, 15),
    (0x8E2F, 8),
    (0x4E38, 2),
    (0x8E3B, 2),
    (0x4E3E, 0),
    (0x8E3F, 0),
    (0x4E40, 15),
    (0x8E50, 0),
    (0x4E51, 15),
    (0x4E61, 2),
    (0x8E64, 1),
    (0x4EDB, 0),
    (0x8EDC, 1),
    (0x4EF6, 0),
    (0x4EFB, 0),
    (0x4EFD, 0),
    (0x8EFE, 0),
    (0x0F04, 3),
    (0x0F0C, 5),
    (0x0F30, 14),
    (0x0F40, 12),
    (0x0F60, 5),
    (0x0F6A, 3),
    (0x0F70, 3),
    (0x0F7A, 3),
    (0x0F80, 7),
    (0x0F95, 0),
    (0x0FA0, 15),
    (0x0FB8, 0),
    (0x1021, 0),
    (0x5028, 0),
    (0x102B, 0),
    (0x102D, 0),
    (0x505B, 0),
    (0x507B, 1),
    (0x10AC, 0),
    (0x10B1, 0),
    (0x1331, 0),
    (0x5416, 0),
    (0x1418, 0),
    (0x141C, 0),
    (0x1600, 1),
    (0x1603, 1),
    (0x1607, 0),
    (0xD60F, 2),
    (0x1612, 1),
    (0x161F, 3),
    (0x962B, 8),
    (0x1634, 1),
    (0x163C, 0),
    (0x16BE, 1),
    (0xD6C0, 3),
    (0x5708, 0),
    (0x570A, 0),
    (0x1710, 0),
    (0x1719, 0),
    (0x5729, 0),
    (0x5B72, 0),
    (0x1B95, 1),
    (0x1BC0, 0),
    (0x5BE6, 0),
    (0x5BE8, 0),
    (0x5BEA, 0),
    (0x5BEC, 0),
    (0x5BEE, 0),
    (0x5D80, 0),
    (0x5D83, 0),
    (0x5D85, 0),
    (0x5D87, 0),
    (0x5D89, 0),
    (0x5D8B, 0),
    (0x5D8D, 0),
    (0x5D8F, 0),
    (0x5D91, 0),
    (0x5D93, 0),
    (0x5D95, 0),
    (0x5D97, 0),
    (0x5D99, 0),
    (0x1D9B, 15),
    (0x1DAB, 4),
    (0x5DD8, 0),
    (0x5DDA, 0),
    (0x5DFC, 0),
    (0xDE00, 10),
    (0x9E0B, 15),
    (0x9E1B, 1),
    (0xDE1D, 1),
    (0x1EEC, 1),
    (0xDEFC, 0),
    (0xDEFF, 0),
    (0x2021, 1),
    (0x2025, 2),
    (0x6029, 0),
    (0x605D, 0),
    (0xA05E, 1),
    (0x2060, 0),
    (0x607C, 1),
    (0xA07E, 0),
    (0x20A8, 0),
    (0xA0AF, 0),
    (0x20B0, 0),
    (0x20B2, 2),
    (0x20B8, 1),
    (0xA2C6, 1),
    (0xA2C9, 0),
    (0x22CA, 1),
    (0xA2CD, 0),
    (0x22D8, 2),
    (0xA2DC, 0),
    (0x22DD, 0),
    (0xA2F7, 0),
    (0xA302, 0),
    (0x2311, 0),
    (0x2320, 0),
    (0x2325, 0),
    (0x2327, 0),
    (0x2331, 0),
    (0x6416, 0),
    (0x2419, 2),
    (0x241D, 2),
    (0x2432, 5),
    (0xA43E, 0),
    (0x2457, 0),
    (0x24DB, 1),
    (0x6709, 0),
    (0x670B, 0),
    (0xA722, 1),
    (0x672A, 0),
    (0xA7B4, 1),
    (0x27CD, 0),
    (0xA7DC, 5),
    (0x6B73, 0),
    (0x6BE7, 0),
    (0x6BE9, 0),
    (0x6BEB, 0),
    (0x6BED, 0),
    (0x6BEF, 0),
    (0x6D80, 0),
    (0x6D84, 0),
    (0x6D86, 0),
    (0x6D88, 0),
    (0x6D8A, 0),
    (0x6D8C, 0),
    (0x6D8E, 0),
    (0x6D90, 0),
    (0x6D92, 0),
    (0x6D94, 0),
    (0x6D96, 0),
    (0x6D98, 1),
    (0x6DD9, 0),
    (0x6DDB, 0),
    (0x6DFD, 0),
];

/// The dictionary entry of an operator's text in a form, if it has one (Blink's
/// `FindCategory`).
fn category(content: &str, form: Form) -> Option<Category> {
    let mut characters = content.chars();
    let first = characters.next()?;
    let second = characters.next();
    if characters.next().is_some() {
        return None;
    }
    let key: u32 = match second {
        None => {
            let code = first as u32;
            // Characters beyond the BMP take two UTF-16 units: Blink looks at them as pairs.
            if code > 0xFFFF {
                if code == 0x1EEF0 || code == 0x1EEF1 {
                    return (form == Form::Postfix).then_some(Category::I);
                }
                return None;
            }
            if (0x0320..=0x03FF).contains(&code) {
                return None;
            }
            code
        }
        Some(second) => {
            if matches!(second, '\u{0338}' | '\u{20D2}') && (first as u32) <= 0xFFFF {
                first as u32
            } else {
                let index = TWO_ASCII.iter().position(|pair| *pair == content)?;
                0x0320 + u32::try_from(index).ok()?
            }
        }
    };
    if key == 0 {
        return None;
    }
    // Categories the compact dictionary does not encode.
    if form == Form::Infix && (key == 0x7C || key == 0x223C) {
        return Some(Category::ForceDefault);
    }
    if form == Form::Prefix
        && ((0x2145..=0x2146).contains(&key) || key == 0x2202 || (0x221A..=0x221C).contains(&key))
    {
        return Some(Category::L);
    }
    if form == Form::Infix && matches!(key, 0x2C | 0x3A | 0x3B) {
        return Some(Category::M);
    }
    let mut key = key;
    if (0x2000..=0x2BFF).contains(&key) {
        key -= 0x1C00;
    } else if key > 0x03FF {
        return None;
    }
    match form {
        Form::Prefix => key |= 0x1000,
        Form::Postfix => key |= 0x2000,
        Form::Infix => {}
    }
    let index = DICTIONARY.partition_point(|&(entry, _)| u32::from(entry & 0x3FFF) <= key);
    let &(entry, delta) = DICTIONARY.get(index.checked_sub(1)?)?;
    if key > u32::from(entry & 0x3FFF) + u32::from(delta) {
        return None;
    }
    Some(match entry >> 12 {
        0x0 => Category::A,
        0x4 => Category::B,
        0x8 => Category::C,
        0x1 | 0x2 | 0xC => Category::Dek,
        0x5 | 0x6 => Category::FG,
        0x9 => Category::H,
        0xA => Category::I,
        0xD => Category::J,
        _ => return None,
    })
}

/// The dictionary properties of an operator with the text `content`: those of its `form`
/// if given, else of the form its position gives, else of the first other form the dictionary
/// lists it in (`MathML` Core, §3.2.4.2).
pub(crate) fn lookup(content: &str, form: Option<Form>, position: Form) -> Entry {
    if content.is_empty() {
        return Entry::DEFAULT;
    }
    let wanted = form.unwrap_or(position);
    if let Some(category) = category(content, wanted) {
        return category.entry();
    }
    if form.is_none() {
        for fallback in [Form::Infix, Form::Postfix, Form::Prefix] {
            if fallback != wanted
                && let Some(category) = category(content, fallback)
            {
                return category.entry();
            }
        }
    }
    Entry::DEFAULT
}

/// The characters whose stretchy operators stretch along the inline axis (Blink's
/// `kStretchyOperatorWithInlineAxis`); all others stretch along the block axis.
const INLINE_AXIS: [u16; 108] = [
    0x003D, 0x005E, 0x005F, 0x007E, 0x00AF, 0x02C6, 0x02C7, 0x02C9, 0x02CD, 0x02DC, 0x02F7, 0x0302, 0x0332,
    0x203E, 0x20D0, 0x20D1, 0x20D6, 0x20D7, 0x20E1, 0x2190, 0x2192, 0x2194, 0x2198, 0x2199, 0x219C, 0x219D,
    0x219E, 0x21A0, 0x21A2, 0x21A3, 0x21A4, 0x21A6, 0x21A9, 0x21AA, 0x21AB, 0x21AC, 0x21AD, 0x21B4, 0x21B9,
    0x21BC, 0x21BD, 0x21C0, 0x21C1, 0x21C4, 0x21C6, 0x21C7, 0x21C9, 0x21CB, 0x21CC, 0x21D0, 0x21D2, 0x21D4,
    0x21DA, 0x21DB, 0x21DC, 0x21DD, 0x21E0, 0x21E2, 0x21E4, 0x21E5, 0x21E6, 0x21E8, 0x21F0, 0x21F6, 0x21FD,
    0x21FE, 0x21FF, 0x23B4, 0x23B5, 0x23DC, 0x23DD, 0x23DE, 0x23DF, 0x23E0, 0x23E1, 0x2500, 0x27F5, 0x27F6,
    0x27F7, 0x27F8, 0x27F9, 0x27FA, 0x27FB, 0x27FC, 0x27FD, 0x27FE, 0x27FF, 0x290C, 0x290D, 0x290E, 0x290F,
    0x2910, 0x294E, 0x2950, 0x2952, 0x2953, 0x2956, 0x2957, 0x295A, 0x295B, 0x295E, 0x295F, 0x2B45, 0x2B46,
    0xFE35, 0xFE36, 0xFE37, 0xFE38,
];

/// Whether an operator of a character stretches along the block axis (Blink's
/// `IsVerticalMathCharacter`).
pub(crate) fn is_vertical(character: char) -> bool {
    let code = character as u32;
    code != 0x1EEF0
        && code != 0x1EEF1
        && u16::try_from(code).map_or(true, |code| INLINE_AXIS.binary_search(&code).is_err())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_operators_by_form() {
        // `+` is an infix operator with medium spaces; `=` is not in the dictionary and takes
        // thick ones.
        assert_eq!(lookup("+", None, Form::Infix), Category::B.entry());
        assert_eq!(lookup("=", None, Form::Infix), Entry::DEFAULT);
        // Parentheses are stretchy and symmetric fences without spacing.
        let fence = lookup("(", None, Form::Prefix);
        assert!(fence.stretchy && fence.symmetric && fence.lspace == 0);
        assert_eq!(lookup(")", None, Form::Postfix), fence);
        // A sum is a large operator with movable limits, an integral one without.
        let sum = lookup("∑", None, Form::Prefix);
        assert!(sum.largeop && sum.movablelimits && sum.lspace == 3);
        let integral = lookup("∫", None, Form::Prefix);
        assert!(integral.largeop && !integral.movablelimits);
        // A comma has space after it only.
        assert_eq!(lookup(",", None, Form::Infix), Category::M.entry());
        // An infix bar takes the default properties, a prefix one stretches.
        assert_eq!(lookup("|", None, Form::Infix), Entry::DEFAULT);
        assert!(lookup("|", None, Form::Prefix).stretchy);
        // `(` between operands falls back to its prefix entry.
        assert!(lookup("(", None, Form::Infix).stretchy);
        // An explicit form does not fall back.
        assert_eq!(lookup("(", Some(Form::Infix), Form::Infix), Entry::DEFAULT);
        // Pairs of ASCII characters have entries of their own.
        assert_eq!(lookup("**", None, Form::Infix), Category::C.entry());
        assert_eq!(lookup("sin", None, Form::Prefix), Entry::DEFAULT);
    }

    #[test]
    fn knows_the_stretch_axis() {
        assert!(is_vertical('('));
        assert!(is_vertical('∑'));
        assert!(!is_vertical('→'));
        assert!(!is_vertical('\u{23DE}'));
    }
}
