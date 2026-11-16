use std::fmt;

/// How serious a diagnostic is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// A message about the input, with its source position.
#[derive(Clone, Debug, PartialEq, Eq)]
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
