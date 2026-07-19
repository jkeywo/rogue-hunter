//! A minimal RFC 4180 reader for the string table.
//!
//! Hand-rolled rather than pulled from a crate: the format is entirely under
//! our control, the WASM client is built for size, and a state machine this
//! small can be tested exhaustively against the cases that actually bite —
//! quoted commas, doubled quotes, and the CRLF the content files use.

/// Split CSV text into records. Every record is a `Vec<String>` of fields with
/// quoting resolved; the header, if any, is just the first record.
///
/// Errors carry a 1-based line number so a malformed table points the author
/// at the row to fix.
pub fn parse(source: &str) -> Result<Vec<Vec<String>>, String> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    // Tracks whether the current field began with a quote, so that stray text
    // after a closing quote can be rejected rather than silently joined.
    let mut closed = false;
    let mut line = 1usize;
    let mut chars = source.chars().peekable();

    while let Some(ch) = chars.next() {
        if quoted {
            match ch {
                '"' => {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        field.push('"');
                    } else {
                        quoted = false;
                        closed = true;
                    }
                }
                '\n' => {
                    line += 1;
                    field.push('\n');
                }
                _ => field.push(ch),
            }
            continue;
        }

        match ch {
            '"' if field.is_empty() && !closed => quoted = true,
            '"' => return Err(format!("line {line}: unexpected quote inside a bare field")),
            ',' => {
                record.push(std::mem::take(&mut field));
                closed = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                line += 1;
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
                closed = false;
            }
            _ if closed => {
                return Err(format!("line {line}: text after a closing quote"));
            }
            _ => field.push(ch),
        }
    }

    if quoted {
        return Err(format!("line {line}: unterminated quoted field"));
    }
    // A trailing newline leaves nothing pending; anything else is a last row.
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn rows(source: &str) -> Vec<Vec<String>> {
        parse(source).expect("parses")
    }

    #[test]
    fn plain_fields_split_on_commas() {
        assert_eq!(rows("a,b,c"), vec![vec!["a", "b", "c"]]);
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
    fn apostrophes_need_no_escaping() {
        assert_eq!(
            rows("id,the wolf's den"),
            vec![vec!["id", "the wolf's den"]]
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
    fn a_trailing_newline_does_not_add_an_empty_record() {
        assert_eq!(rows("a,b\r\n"), vec![vec!["a", "b"]]);
    }

    #[test]
    fn a_quoted_field_may_span_lines() {
        assert_eq!(
            rows("id,\"one\ntwo\"\nnext,x"),
            vec![vec!["id", "one\ntwo"], vec!["next", "x"]]
        );
    }

    #[test]
    fn an_empty_source_yields_no_records() {
        assert_eq!(parse("").expect("parses"), Vec::<Vec<String>>::new());
    }

    #[test]
    fn an_unterminated_quote_is_an_error() {
        assert!(parse("id,\"never closed").is_err());
    }

    #[test]
    fn text_after_a_closing_quote_is_an_error() {
        assert!(parse(r#"id,"closed"trailing"#).is_err());
    }
}
