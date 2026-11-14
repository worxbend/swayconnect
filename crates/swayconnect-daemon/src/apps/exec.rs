//! Desktop Entry Specification value escapes, the `Exec` quoting rules and field codes
//! ("The Exec key"). The result is an argv the daemon owns: it is never given to a shell.
//!
//! The reader is strict where the spec says "must": a reserved character outside quotes,
//! an unknown escape inside quotes, an unterminated quote or an unknown field code refuses
//! the whole entry, so the daemon never guesses what an entry meant to run.

/// Characters that the spec says must be quoted inside an argument.
const RESERVED: &[char] = &[
    '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(', ')', '`',
];
/// Characters that need a backslash inside a quoted argument.
const QUOTED_ESCAPABLE: &[char] = &['"', '`', '$', '\\'];
/// Every field code; each one is removed (the phone never passes files or URLs).
const FIELD_CODES: &[char] = &[
    'f', 'F', 'u', 'U', 'd', 'D', 'n', 'N', 'i', 'c', 'k', 'v', 'm',
];
const URL_FIELD_CODES: &[char] = &['u', 'U'];

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ExecError {
    #[error("Exec names no program")]
    Empty,
    #[error("Exec has an unterminated quote")]
    UnterminatedQuote,
    #[error("Exec has a quote inside an unquoted argument or text after a closing quote")]
    MisplacedQuote,
    #[error("Exec has an invalid escape inside quotes")]
    BadEscape,
    #[error("Exec has the reserved character {0:?} outside quotes")]
    Reserved(char),
    #[error("Exec has an unknown field code")]
    UnknownFieldCode,
}

/// An `Exec` value split into arguments, with every field code removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecLine {
    pub argv: Vec<String>,
    /// `%u` or `%U` was present: the program takes URLs.
    pub takes_urls: bool,
}

/// Applies the escapes of type `string`: `\s`, `\n`, `\t`, `\r` and `\\`. Any other
/// backslash stays, because the `Exec` quoting rules give it a meaning later.
pub fn unescape_value(raw: &str) -> String {
    unescape(raw, false).concat()
}

/// A `;`-separated list value (`Keywords`, `OnlyShowIn`): `\;` is a literal `;`. Empty
/// items, such as the one after a trailing `;`, are dropped.
pub fn split_list(raw: &str) -> Vec<String> {
    unescape(raw, true)
        .into_iter()
        .filter(|item| !item.is_empty())
        .collect()
}

/// Splits `raw` (the file's `Exec` value, before unescaping) into an argv.
pub fn parse_exec(raw: &str) -> Result<ExecLine, ExecError> {
    let mut splitter = Splitter::default();
    let text = unescape_value(raw);
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        splitter.take(character, &mut chars)?;
    }
    splitter.finish()
}

fn unescape(raw: &str, split_on_semicolon: bool) -> Vec<String> {
    let mut items = vec![String::new()];
    let mut chars = raw.chars();
    while let Some(character) = chars.next() {
        let current = items.last_mut().expect("items is never empty");
        match character {
            '\\' => match chars.next() {
                Some('s') => current.push(' '),
                Some('n') => current.push('\n'),
                Some('t') => current.push('\t'),
                Some('r') => current.push('\r'),
                Some('\\') => current.push('\\'),
                Some(';') if split_on_semicolon => current.push(';'),
                Some(other) => current.extend(['\\', other]),
                None => current.push('\\'),
            },
            ';' if split_on_semicolon => items.push(String::new()),
            _ => current.push(character),
        }
    }
    items
}

#[derive(Debug, Default)]
struct Splitter {
    argv: Vec<String>,
    word: Option<Word>,
    takes_urls: bool,
}

#[derive(Debug, Default)]
struct Word {
    text: String,
    quoted: bool,
    /// The closing quote was read; only a space may follow.
    closed: bool,
    had_field_code: bool,
}

impl Splitter {
    fn take(
        &mut self,
        character: char,
        rest: &mut impl Iterator<Item = char>,
    ) -> Result<(), ExecError> {
        let in_quotes = self.word.as_ref().is_some_and(|w| w.quoted && !w.closed);
        match character {
            ' ' if !in_quotes => self.end_word(),
            '%' => self.field_code(rest.next())?,
            '"' if in_quotes => self.word_mut().closed = true,
            '"' if self.word.is_none() => {
                self.word = Some(Word {
                    quoted: true,
                    ..Word::default()
                });
            }
            '"' => return Err(ExecError::MisplacedQuote),
            '\\' if in_quotes => match rest.next() {
                Some(escaped) if QUOTED_ESCAPABLE.contains(&escaped) => self.push(escaped)?,
                _ => return Err(ExecError::BadEscape),
            },
            _ if in_quotes => self.push(character)?,
            _ if RESERVED.contains(&character) => return Err(ExecError::Reserved(character)),
            _ => self.push(character)?,
        }
        Ok(())
    }

    fn field_code(&mut self, code: Option<char>) -> Result<(), ExecError> {
        match code {
            Some('%') => self.push('%'),
            Some(code) if FIELD_CODES.contains(&code) => {
                self.takes_urls |= URL_FIELD_CODES.contains(&code);
                self.check_open()?;
                self.word_mut().had_field_code = true;
                Ok(())
            }
            _ => Err(ExecError::UnknownFieldCode),
        }
    }

    fn push(&mut self, character: char) -> Result<(), ExecError> {
        self.check_open()?;
        self.word_mut().text.push(character);
        Ok(())
    }

    fn check_open(&self) -> Result<(), ExecError> {
        match &self.word {
            Some(word) if word.closed => Err(ExecError::MisplacedQuote),
            _ => Ok(()),
        }
    }

    fn word_mut(&mut self) -> &mut Word {
        self.word.get_or_insert_with(Word::default)
    }

    /// A word made only of field codes disappears; an explicit `""` stays an empty argument.
    fn end_word(&mut self) {
        if let Some(word) = self.word.take()
            && !(word.text.is_empty() && word.had_field_code)
        {
            self.argv.push(word.text);
        }
    }

    fn finish(mut self) -> Result<ExecLine, ExecError> {
        if self.word.as_ref().is_some_and(|w| w.quoted && !w.closed) {
            return Err(ExecError::UnterminatedQuote);
        }
        self.end_word();
        if self.argv.first().is_none_or(String::is_empty) {
            return Err(ExecError::Empty);
        }
        Ok(ExecLine {
            argv: self.argv,
            takes_urls: self.takes_urls,
        })
    }
}

#[cfg(test)]
#[path = "exec_tests.rs"]
mod tests;
