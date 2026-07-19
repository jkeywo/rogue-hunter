//! The localisation string table.
//!
//! Every string the player reads lives in `content/strings.csv`, keyed by a
//! stable id, with a context note telling a writer or translator where the
//! line appears. Content TOML and Rust code both hold ids; the text itself is
//! resolved here at the point of display.
//!
//! The table is deliberately *not* part of `content_fingerprint`. Rewriting a
//! line, or translating it, must not invalidate a share code. That buys a
//! rule, which validation and the generator's authored placement now enforce:
//! anything the RNG indexes or the simulation branches on stays in TOML.

pub(crate) mod csv;

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::catalogue::ContentError;

/// A reference to a row of the string table.
///
/// A newtype so the type system keeps ids and prose apart: a field of this
/// type is a lookup key, never something to show a player.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct StringId(pub String);

impl StringId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for StringId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for StringId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringRow {
    /// Where this line appears and how it is used, for whoever writes or
    /// translates it. Validation requires it to be non-empty.
    pub context: String,
    pub english: String,
}

/// Every authored string, keyed by id.
///
/// Cheap to clone: `Catalogue` is cloned per run and repeatedly while
/// searching for a viable seed, so the rows sit behind an `Arc`.
#[derive(Debug, Clone, Default)]
pub struct StringTable {
    rows: Arc<BTreeMap<String, StringRow>>,
}

impl StringTable {
    /// Parse the table from CSV text with an `id,context,english` header.
    pub fn parse(source: &str) -> Result<Self, ContentError> {
        let invalid = |issue: String| ContentError::Invalid {
            issues: vec![format!("strings.csv: {issue}")],
        };
        let records = csv::parse(source).map_err(invalid)?;
        let mut records = records.into_iter();
        let header = records
            .next()
            .ok_or_else(|| invalid("the file is empty".to_owned()))?;
        if header != ["id", "context", "english"] {
            return Err(invalid(format!(
                "expected an 'id,context,english' header, found {header:?}"
            )));
        }

        let mut rows = BTreeMap::new();
        for (index, record) in records.enumerate() {
            // +2: one for the header, one to count from 1.
            let line = index + 2;
            if record.len() != 3 {
                return Err(invalid(format!(
                    "line {line}: expected 3 fields, found {}",
                    record.len()
                )));
            }
            let [id, context, english]: [String; 3] =
                record.try_into().expect("length checked above");
            if id.is_empty() {
                return Err(invalid(format!("line {line}: empty id")));
            }
            if rows
                .insert(id.clone(), StringRow { context, english })
                .is_some()
            {
                return Err(invalid(format!("line {line}: duplicate id '{id}'")));
            }
        }
        Ok(Self {
            rows: Arc::new(rows),
        })
    }

    /// Resolve an id, or a loud sentinel if it is missing.
    ///
    /// Deliberately not a panic: an unknown id in the WASM client should show
    /// a visible defect, not a blank screen. Content-side ids are checked at
    /// load by `validate`, and tests assert nothing rendered ever contains
    /// the sentinel, so this only ever fires on a code-side typo.
    pub fn get(&self, id: &StringId) -> &str {
        self.try_get(id.as_str()).unwrap_or_else(|| {
            debug_assert!(false, "missing string id '{id}'");
            "[!missing]"
        })
    }

    pub fn try_get(&self, id: &str) -> Option<&str> {
        self.rows.get(id).map(|row| row.english.as_str())
    }

    pub fn row(&self, id: &str) -> Option<&StringRow> {
        self.rows.get(id)
    }

    /// Resolve an id and substitute `{name}`-style placeholders.
    ///
    /// Matches the substitution the content files already document, so a
    /// translator sees the same `{npc}` and `{clue}` markers they do today.
    pub fn fill(&self, id: &StringId, args: &[(&str, &str)]) -> String {
        let mut text = self.get(id).to_owned();
        for (key, value) in args {
            text = text.replace(&format!("{{{key}}}"), value);
        }
        text
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.rows.keys().map(String::as_str)
    }

    pub fn rows(&self) -> impl Iterator<Item = (&str, &StringRow)> {
        self.rows.iter().map(|(id, row)| (id.as_str(), row))
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "id,context,english\r\nui.a,Somewhere,[Hello {who}.]\r\n";

    #[test]
    fn parses_a_table_and_resolves_an_id() {
        let table = StringTable::parse(TABLE).expect("parses");
        assert_eq!(table.len(), 1);
        assert_eq!(table.try_get("ui.a"), Some("[Hello {who}.]"));
        assert_eq!(table.row("ui.a").expect("row").context, "Somewhere");
    }

    #[test]
    fn fill_substitutes_named_placeholders() {
        let table = StringTable::parse(TABLE).expect("parses");
        assert_eq!(
            table.fill(&StringId::from("ui.a"), &[("who", "the miller")]),
            "[Hello the miller.]"
        );
    }

    #[test]
    fn a_wrong_header_is_refused() {
        assert!(StringTable::parse("id,english\r\nui.a,x\r\n").is_err());
    }

    #[test]
    fn a_duplicate_id_is_refused() {
        let source = "id,context,english\r\nui.a,c,[one]\r\nui.a,c,[two]\r\n";
        assert!(StringTable::parse(source).is_err());
    }

    #[test]
    fn a_ragged_row_is_refused() {
        assert!(StringTable::parse("id,context,english\r\nui.a,only-two\r\n").is_err());
    }
}
