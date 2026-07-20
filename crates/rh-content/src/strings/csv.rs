//! The string table's CSV reader.
//!
//! The state machine lives in `vellum-strings`, shared with the other game
//! that wrote the same one. This keeps the error type rogue-hunter's content
//! loader expects, and keeps the cases that actually bite asserted here as
//! well as there: an engine change that broke quoting should fail in the game
//! that depends on it, not only in the crate that made it.

/// Split CSV text into records. Every record is a `Vec<String>` of fields with
/// quoting resolved; the header, if any, is just the first record.
///
/// Errors carry a 1-based line number so a malformed table points the author
/// at the row to fix.
pub fn parse(source: &str) -> Result<Vec<Vec<String>>, String> {
    vellum_strings::parse_csv(source).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn rows(source: &str) -> Vec<Vec<String>> {
        parse(source).expect("parses")
    }

    #[test]
    fn a_quoted_field_may_contain_a_comma() {
        assert_eq!(
            rows(r#"id,"one, two",z"#),
            vec![vec!["id", "one, two", "z"]]
        );
    }

    #[test]
    fn a_doubled_quote_is_one_literal_quote() {
        assert_eq!(
            rows(r#"id,"she said ""no""","#),
            vec![vec!["id", r#"she said "no""#, ""]]
        );
    }

    #[test]
    fn crlf_and_lf_both_end_a_record() {
        // Content files in this repo are CRLF; the table must not care.
        assert_eq!(
            rows("a,b\r\nc,d\ne,f"),
            vec![vec!["a", "b"], vec!["c", "d"], vec!["e", "f"],]
        );
    }

    #[test]
    fn an_unterminated_quote_is_an_error() {
        assert!(parse("id,\"never closed").is_err());
    }
}
