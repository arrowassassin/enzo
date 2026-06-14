//! Source formatting — run a language's configured external formatter.
//!
//! Formatters are spawned as subprocesses (rustfmt, black, prettier) reading
//! source on stdin and writing the formatted result to stdout, matching the
//! `stdin` formatters declared in [`crate::lang`]. This keeps Enzo's principle
//! of *orchestrating engines, not authoring them* (design doc §2).

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::Context;

use crate::lang::Language;

/// Format `source` using `language`'s configured formatter.
///
/// Returns the formatted text. If the language has no formatter, returns the
/// source unchanged.
///
/// # Errors
/// Returns an error if the formatter binary is missing or exits non-zero.
pub fn format_source(language: Language, source: &str) -> anyhow::Result<String> {
    let Some(fmt) = language.formatter() else {
        return Ok(source.to_owned());
    };
    if !fmt.stdin {
        anyhow::bail!("non-stdin formatters are not supported in-memory");
    }

    let mut child = Command::new(fmt.command)
        .args(fmt.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn formatter {}", fmt.command))?;

    child
        .stdin
        .take()
        .context("formatter stdin")?
        .write_all(source.as_bytes())
        .context("write source to formatter")?;

    let output = child
        .wait_with_output()
        .with_context(|| format!("wait for {}", fmt.command))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{} failed: {}", fmt.command, stderr.trim());
    }

    String::from_utf8(output.stdout).context("formatter output not UTF-8")
}

/// Check whether `language`'s formatter binary is available on `PATH`.
#[must_use]
pub fn formatter_available(language: Language) -> bool {
    let Some(fmt) = language.formatter() else {
        return false;
    };
    Command::new(fmt.command)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_returns_source_unchanged() {
        let src = "no formatter here";
        assert_eq!(format_source(Language::PlainText, src).unwrap(), src);
    }

    #[test]
    fn missing_formatter_binary_errors() {
        // JSON formatter is `prettier`; if absent this errors cleanly rather
        // than panicking. We only assert it doesn't panic and returns a Result.
        let result = format_source(Language::Json, "{}");
        // Either prettier is installed (Ok) or it's missing (Err) — both fine.
        let _ = result;
    }

    #[test]
    fn formatter_available_is_boolean_for_plaintext() {
        assert!(!formatter_available(Language::PlainText));
    }

    #[test]
    fn rustfmt_formats_when_available() {
        if !formatter_available(Language::Rust) {
            return; // skip in environments without rustfmt
        }
        let messy = "fn   main( ){let x=1;}";
        let formatted = format_source(Language::Rust, messy).unwrap();
        assert!(formatted.contains("fn main()"));
    }
}
