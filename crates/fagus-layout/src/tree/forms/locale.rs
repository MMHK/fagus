//! The texts that form controls show of their own and the way they write dates, times and
//! numbers, in the languages of Chrome's user interface.
//!
//! Chrome takes these from the language of its user interface (and macOS's formats for that
//! language), never from the document. Fagus takes the language of the control instead: its
//! `lang`, inherited from its ancestors, else the language the engine is given, else American
//! English, Chrome's default.
//!
//! The texts were measured in Chrome 157 on macOS with its user interface in each language; they
//! are Chromium's translations (`blink_strings_*.xtb`, `components/strings`). The formats are
//! those Blink's `LocaleMac` reads from `NSDateFormatter` and `NSNumberFormatter`: the short date
//! and time, the month template `yyyyMMMM`, the decimal separator.

/// How a control writes things in one language.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct UiLocale {
    /// The language tag the entry is for.
    pub tag: &'static str,
    /// The label of `input type=submit` (and of `type=image` without `alt`).
    pub submit: &'static str,
    /// The label of `input type=reset`.
    pub reset: &'static str,
    /// The button of `input type=file`.
    pub choose_file: &'static str,
    /// The button of `input type=file multiple`.
    pub choose_files: &'static str,
    /// The text beside that button.
    pub no_file: &'static str,
    /// What a `<select multiple size=1>` shows unless one option is selected: `{n}` for the
    /// number of selected options.
    pub selected: &'static str,
    /// The summary of a `<details>` without one.
    pub details: &'static str,
    /// The placeholders of the day, month and year fields of a date.
    pub placeholders: [&'static str; 3],
    /// A date: `{d}`, `{M}` and `{y}` stand for the fields of the day, month and year.
    pub date: &'static str,
    /// The text between the date and the time of `datetime-local`.
    pub date_time: &'static str,
    /// The labels of the morning and the afternoon where hours go up to twelve; `None` where
    /// they go up to 23.
    pub am_pm: Option<[&'static str; 2]>,
    /// A month (`input type=month`): `{M}` for the name of the month, `{y}` for the year.
    pub month: &'static str,
    /// The names of the months, as `month` writes them.
    pub months: [&'static str; 12],
    /// A week (`input type=week`): `{w}` for the week, `{y}` for the year.
    pub week: &'static str,
    /// The decimal separator of numbers.
    pub decimal: char,
}

const ENGLISH_MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const PORTUGUESE_MONTHS: [&str; 12] = [
    "janeiro",
    "fevereiro",
    "março",
    "abril",
    "maio",
    "junho",
    "julho",
    "agosto",
    "setembro",
    "outubro",
    "novembro",
    "dezembro",
];

/// American English, Chrome's default.
pub(crate) const EN_US: UiLocale = UiLocale {
    tag: "en-US",
    submit: "Submit",
    reset: "Reset",
    choose_file: "Choose File",
    choose_files: "Choose Files",
    no_file: "No file chosen",
    selected: "{n} selected",
    details: "Details",
    placeholders: ["dd", "mm", "yyyy"],
    date: "{M}/{d}/{y}",
    date_time: ", ",
    am_pm: Some(["AM", "PM"]),
    month: "{M} {y}",
    months: ENGLISH_MONTHS,
    week: "Week {w}, {y}",
    decimal: '.',
};

const EN_GB: UiLocale = UiLocale {
    tag: "en-GB",
    choose_file: "Choose file",
    choose_files: "Choose files",
    date: "{d}/{M}/{y}",
    am_pm: None,
    ..EN_US
};

/// English in Australia: Chrome's British texts, macOS's dates (`d/M/y`) and twelve hours with
/// lowercase labels.
const EN_AU: UiLocale = UiLocale {
    tag: "en-AU",
    am_pm: Some(["am", "pm"]),
    ..EN_GB
};

/// English in India: Chrome's British texts, macOS's dates (`dd/MM/yy`) and twelve hours.
const EN_IN: UiLocale = UiLocale {
    tag: "en-IN",
    am_pm: Some(["AM", "PM"]),
    ..EN_GB
};

const DE: UiLocale = UiLocale {
    tag: "de",
    submit: "Senden",
    reset: "Zurücksetzen",
    choose_file: "Datei auswählen",
    choose_files: "Dateien auswählen",
    no_file: "Keine ausgewählt",
    selected: "{n} ausgewählt",
    details: "Details",
    placeholders: ["tt", "mm", "jjjj"],
    date: "{d}.{M}.{y}",
    date_time: ", ",
    am_pm: None,
    month: "{M} {y}",
    months: [
        "Januar",
        "Februar",
        "März",
        "April",
        "Mai",
        "Juni",
        "Juli",
        "August",
        "September",
        "Oktober",
        "November",
        "Dezember",
    ],
    week: "Woche {w}, {y}",
    decimal: ',',
};

const FR: UiLocale = UiLocale {
    tag: "fr",
    submit: "Envoyer",
    reset: "Réinitialiser",
    choose_file: "Choisir un fichier",
    choose_files: "Sélect. fichiers",
    no_file: "Aucun fichier choisi",
    selected: "{n} élément(s) sélectionné(s)",
    details: "Détails",
    placeholders: ["jj", "mm", "aaaa"],
    date: "{d}/{M}/{y}",
    date_time: " ",
    am_pm: None,
    month: "{M} {y}",
    months: [
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ],
    week: "Semaine {w}, {y}",
    decimal: ',',
};

const ES: UiLocale = UiLocale {
    tag: "es",
    submit: "Enviar",
    reset: "Restablecer",
    choose_file: "Seleccionar archivo",
    choose_files: "Elegir archivos",
    no_file: "Ningún archivo seleccionado",
    selected: "{n} seleccionados",
    details: "Detalles",
    placeholders: ["dd", "mm", "aaaa"],
    date: "{d}/{M}/{y}",
    date_time: ", ",
    am_pm: None,
    month: "{M} de {y}",
    months: [
        "enero",
        "febrero",
        "marzo",
        "abril",
        "mayo",
        "junio",
        "julio",
        "agosto",
        "septiembre",
        "octubre",
        "noviembre",
        "diciembre",
    ],
    week: "Semana {w}, {y}",
    decimal: ',',
};

/// Latin American Spanish (`es-419`, also Mexico): Chrome's texts of that name, twelve hours
/// with macOS's `a.m.` and `p.m.`, and a decimal point.
const ES_419: UiLocale = UiLocale {
    tag: "es-419",
    no_file: "Sin archivos seleccionados",
    selected: "{n} elementos seleccionados",
    am_pm: Some(["a.m.", "p.m."]),
    decimal: '.',
    ..ES
};

const IT: UiLocale = UiLocale {
    tag: "it",
    submit: "Invia",
    reset: "Reimposta",
    choose_file: "Scegli file",
    choose_files: "Scegli file",
    no_file: "Nessun file selezionato",
    selected: "Elementi selezionati: {n}",
    details: "Dettagli",
    placeholders: ["gg", "mm", "aaaa"],
    date: "{d}/{M}/{y}",
    date_time: ", ",
    am_pm: None,
    month: "{M} {y}",
    months: [
        "gennaio",
        "febbraio",
        "marzo",
        "aprile",
        "maggio",
        "giugno",
        "luglio",
        "agosto",
        "settembre",
        "ottobre",
        "novembre",
        "dicembre",
    ],
    week: "Settimana {w}, {y}",
    decimal: ',',
};

/// French in Canada: the texts of French, macOS's dates year first (`y-MM-dd`).
const FR_CA: UiLocale = UiLocale {
    tag: "fr-CA",
    date: "{y}-{M}-{d}",
    ..FR
};

const NL: UiLocale = UiLocale {
    tag: "nl",
    submit: "Verzenden",
    reset: "Resetten",
    choose_file: "Bestand kiezen",
    choose_files: "Bestanden kiezen",
    no_file: "Geen bestand gekozen",
    selected: "{n} geselecteerd",
    details: "Details",
    placeholders: ["dd", "mm", "jjjj"],
    date: "{d}-{M}-{y}",
    date_time: ", ",
    am_pm: None,
    month: "{M} {y}",
    months: [
        "januari",
        "februari",
        "maart",
        "april",
        "mei",
        "juni",
        "juli",
        "augustus",
        "september",
        "oktober",
        "november",
        "december",
    ],
    week: "Week {w}, {y}",
    decimal: ',',
};

/// Brazilian Portuguese, Chrome's Portuguese.
const PT_BR: UiLocale = UiLocale {
    tag: "pt-BR",
    submit: "Enviar",
    reset: "Redefinir",
    choose_file: "Escolher arquivo",
    choose_files: "Escolher arquivos",
    no_file: "Nenhum arquivo escolhido",
    selected: "{n} selecionados",
    details: "Saiba mais",
    placeholders: ["dd", "mm", "aaaa"],
    date: "{d}/{M}/{y}",
    date_time: ", ",
    am_pm: None,
    month: "{M} de {y}",
    months: PORTUGUESE_MONTHS,
    week: "Semana {w}, {y}",
    decimal: ',',
};

const PT_PT: UiLocale = UiLocale {
    tag: "pt-PT",
    reset: "Repor",
    choose_file: "Escolher ficheiro",
    choose_files: "Escolher Ficheiros",
    no_file: "Nenhum ficheiro selecionado",
    details: "Detalhes",
    week: "Semana {w}, de {y}",
    ..PT_BR
};

const PL: UiLocale = UiLocale {
    tag: "pl",
    submit: "Prześlij",
    reset: "Resetuj",
    choose_file: "Wybierz plik",
    choose_files: "Wybierz pliki",
    no_file: "Nie wybrano pliku",
    selected: "Wybrano: {n}",
    details: "Szczegóły",
    placeholders: ["dd", "mm", "rrrr"],
    date: "{d}.{M}.{y}",
    date_time: ", ",
    am_pm: None,
    month: "{M} {y}",
    // The month stands alone here (`LLLL yyyy`): the nominative.
    months: [
        "styczeń",
        "luty",
        "marzec",
        "kwiecień",
        "maj",
        "czerwiec",
        "lipiec",
        "sierpień",
        "wrzesień",
        "październik",
        "listopad",
        "grudzień",
    ],
    week: "Tydzień {w}, {y}",
    decimal: ',',
};

/// The entry for a language tag (BCP 47, `de`, `de-DE`, `en_GB`), if Fagus has its language.
/// English is American but in Great Britain, Australia and India, Spanish Latin American in
/// Latin America (`419`) and Mexico, French Canadian in Canada, Portuguese Brazilian but in
/// Portugal; other regions take the language's entry.
pub(crate) fn ui_locale(tag: &str) -> Option<&'static UiLocale> {
    let mut subtags = tag.trim().split(['-', '_']).map(str::to_ascii_lowercase);
    let language = subtags.next()?;
    // The region follows the language and an optional script (four letters).
    let region = subtags.find(|subtag| subtag.len() != 4);
    let region = region.as_deref();
    Some(match language.as_str() {
        "en" => match region {
            Some("gb") => &EN_GB,
            Some("au") => &EN_AU,
            Some("in") => &EN_IN,
            _ => &EN_US,
        },
        "de" => &DE,
        "fr" if region == Some("ca") => &FR_CA,
        "fr" => &FR,
        "es" if matches!(region, Some("419" | "mx")) => &ES_419,
        "es" => &ES,
        "it" => &IT,
        "nl" => &NL,
        "pt" if region == Some("pt") => &PT_PT,
        "pt" => &PT_BR,
        "pl" => &PL,
        _ => return None,
    })
}

/// The entry for a control: by its language (`lang`, inherited), else by the language the
/// engine was given, else American English.
pub(crate) fn resolve(lang: Option<&str>, default: Option<&str>) -> &'static UiLocale {
    lang.and_then(ui_locale)
        .or_else(|| default.and_then(ui_locale))
        .unwrap_or(&EN_US)
}

/// A pattern of a [`UiLocale`] in pieces: text, or the letter of a field in braces.
pub(crate) fn pieces(pattern: &'static str) -> impl Iterator<Item = Piece> {
    let mut rest = pattern;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        if let Some(after) = rest.strip_prefix('{')
            && let Some((field, tail)) = after.split_once('}')
            && let Some(letter) = field.chars().next()
        {
            rest = tail;
            return Some(Piece::Field(letter));
        }
        let first = rest.chars().next().map_or(0, char::len_utf8);
        let end = rest[first..].find('{').map_or(rest.len(), |end| end + first);
        let (text, tail) = rest.split_at(end);
        rest = tail;
        Some(Piece::Text(text))
    })
}

/// A piece of a pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Piece {
    Text(&'static str),
    Field(char),
}

/// A number (a valid floating-point number) written as a number field shows it: with the
/// decimal separator of the locale, unless it is in scientific notation (Blink's
/// `NumberInputType::LocalizeValue`).
pub(crate) fn localize_number(value: &str, locale: &UiLocale) -> String {
    if value.contains(['e', 'E']) {
        return value.to_owned();
    }
    value
        .chars()
        .map(|c| if c == '.' { locale.decimal } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_find_their_language() {
        assert_eq!(ui_locale("de").map(|l| l.tag), Some("de"));
        assert_eq!(ui_locale("DE-at").map(|l| l.tag), Some("de"));
        assert_eq!(ui_locale("en").map(|l| l.tag), Some("en-US"));
        assert_eq!(ui_locale("en-CA").map(|l| l.tag), Some("en-US"));
        assert_eq!(ui_locale("en_GB").map(|l| l.tag), Some("en-GB"));
        assert_eq!(ui_locale("pt").map(|l| l.tag), Some("pt-BR"));
        assert_eq!(ui_locale("pt-Latn-PT").map(|l| l.tag), Some("pt-PT"));
        assert_eq!(ui_locale("ja"), None);
        assert_eq!(ui_locale(""), None);
    }

    #[test]
    fn regions_with_their_own_formats() {
        let tag = |tag: &str| ui_locale(tag).map(|l| l.tag);
        assert_eq!(tag("es-419"), Some("es-419"));
        assert_eq!(tag("es-MX"), Some("es-419"));
        assert_eq!(tag("es-ES"), Some("es"));
        assert_eq!(tag("fr-CA"), Some("fr-CA"));
        assert_eq!(tag("fr-FR"), Some("fr"));
        assert_eq!(tag("en-AU"), Some("en-AU"));
        assert_eq!(tag("en_IN"), Some("en-IN"));
        // Chrome has no Australian or Indian English of its own: the texts are British.
        assert_eq!(EN_AU.choose_file, "Choose file");
        assert_eq!(EN_IN.choose_files, "Choose files");
        assert_eq!(ES_419.no_file, "Sin archivos seleccionados");
        assert_eq!(localize_number("1234.5", &ES_419), "1234.5");
        assert_eq!(localize_number("1234.5", &FR_CA), "1234,5");
    }

    #[test]
    fn lang_comes_before_the_option() {
        assert_eq!(resolve(Some("fr"), Some("de")).tag, "fr");
        assert_eq!(resolve(None, Some("de-DE")).tag, "de");
        // A language without an entry, or an empty `lang`, leaves the choice to the option.
        assert_eq!(resolve(Some("ja"), Some("pl")).tag, "pl");
        assert_eq!(resolve(Some(""), Some("nl")).tag, "nl");
        assert_eq!(resolve(None, None).tag, "en-US");
        assert_eq!(resolve(Some("xx"), Some("yy")).tag, "en-US");
    }

    #[test]
    fn texts_per_language() {
        let texts = |tag: &str| {
            let locale = resolve(Some(tag), None);
            (
                locale.submit,
                locale.reset,
                locale.choose_file,
                locale.no_file,
                locale.details,
            )
        };
        assert_eq!(
            texts("en"),
            ("Submit", "Reset", "Choose File", "No file chosen", "Details")
        );
        assert_eq!(
            texts("en-GB"),
            ("Submit", "Reset", "Choose file", "No file chosen", "Details")
        );
        assert_eq!(
            texts("de"),
            (
                "Senden",
                "Zurücksetzen",
                "Datei auswählen",
                "Keine ausgewählt",
                "Details"
            )
        );
        assert_eq!(
            texts("fr"),
            (
                "Envoyer",
                "Réinitialiser",
                "Choisir un fichier",
                "Aucun fichier choisi",
                "Détails"
            )
        );
        assert_eq!(
            texts("es"),
            (
                "Enviar",
                "Restablecer",
                "Seleccionar archivo",
                "Ningún archivo seleccionado",
                "Detalles"
            )
        );
        assert_eq!(
            texts("it"),
            (
                "Invia",
                "Reimposta",
                "Scegli file",
                "Nessun file selezionato",
                "Dettagli"
            )
        );
        assert_eq!(
            texts("nl"),
            (
                "Verzenden",
                "Resetten",
                "Bestand kiezen",
                "Geen bestand gekozen",
                "Details"
            )
        );
        assert_eq!(
            texts("pt"),
            (
                "Enviar",
                "Redefinir",
                "Escolher arquivo",
                "Nenhum arquivo escolhido",
                "Saiba mais"
            )
        );
        assert_eq!(
            texts("pt-PT"),
            (
                "Enviar",
                "Repor",
                "Escolher ficheiro",
                "Nenhum ficheiro selecionado",
                "Detalhes"
            )
        );
        assert_eq!(
            texts("pl"),
            (
                "Prześlij",
                "Resetuj",
                "Wybierz plik",
                "Nie wybrano pliku",
                "Szczegóły"
            )
        );
    }

    #[test]
    fn patterns_split_into_pieces() {
        let week: Vec<Piece> = pieces("Semana {w}, de {y}").collect();
        assert_eq!(
            week,
            [
                Piece::Text("Semana "),
                Piece::Field('w'),
                Piece::Text(", de "),
                Piece::Field('y')
            ]
        );
        assert_eq!(pieces("{d}.{M}.{y}").count(), 5);
    }

    #[test]
    fn numbers_take_the_decimal_separator() {
        assert_eq!(localize_number("-1234.5", &DE), "-1234,5");
        assert_eq!(localize_number("1234.5", &EN_US), "1234.5");
        assert_eq!(localize_number("1.5e3", &DE), "1.5e3");
    }
}
