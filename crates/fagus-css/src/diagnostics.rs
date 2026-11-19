use std::fmt;

/// How serious a diagnostic is.
///
/// Not `#[non_exhaustive]`: the three severities are those of the command line's diagnostics
/// lines and of the worker protocol, and stay the same within a major release, so a `match`
/// over them may list all three. Which severity a diagnostic gets is not stable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// A message about the input, with its source position.
///
/// `#[non_exhaustive]`, so that a field can be added in a minor release: outside this crate, make
/// one with [`Diagnostic::new`] and [`Diagnostic::at`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Diagnostic {
    pub severity: Severity,
    /// A stable code, e.g. `css.parse-error`.
    pub code: &'static str,
    pub message: String,
    /// The file or source the message refers to, e.g. `style.css` or `<style> 2`.
    pub source: String,
    /// 1-based line, 0 if unknown.
    pub line: u32,
    /// 1-based column, 0 if unknown.
    pub column: u32,
}

impl Diagnostic {
    /// A diagnostic of `source` (a file name, `<style> 1`, `html`, `resources`, `pdf`) without a
    /// position: its `line` and `column` are 0.
    pub fn new(
        severity: Severity,
        code: &'static str,
        message: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            code,
            message: message.into(),
            source: source.into(),
            line: 0,
            column: 0,
        }
    }

    /// The diagnostic at the 1-based `line` and `column` of its source, 0 where unknown.
    #[must_use]
    pub fn at(mut self, line: u32, column: u32) -> Self {
        self.line = line;
        self.column = column;
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = match self.severity {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        // Unknown positions are left out.
        write!(f, "{}", self.source)?;
        if self.line > 0 {
            write!(f, ":{}", self.line)?;
            if self.column > 0 {
                write!(f, ":{}", self.column)?;
            }
        }
        write!(f, ": {severity}[{}]: {}", self.code, self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_out_unknown_positions() {
        let diagnostic = |line, column| Diagnostic {
            severity: Severity::Warning,
            code: "css.test",
            message: "message".to_owned(),
            source: "style.css".to_owned(),
            line,
            column,
        };
        assert_eq!(
            diagnostic(3, 7).to_string(),
            "style.css:3:7: warning[css.test]: message"
        );
        assert_eq!(
            diagnostic(3, 0).to_string(),
            "style.css:3: warning[css.test]: message"
        );
        assert_eq!(
            diagnostic(0, 0).to_string(),
            "style.css: warning[css.test]: message"
        );
    }
}
