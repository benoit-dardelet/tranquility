//! Multi-tap (T9-style) keypad text entry.
use super::i18n::{self, Lang};
use super::Key;
use crate::prelude::*;
use crate::util;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Sentence,
    Lower,
    Upper,
    Num,
}

const STAR: &str = "*+=#%&<>;_$€£";
const MULTITAP_MS: u64 = 1000;

#[derive(Clone, Debug)]
pub struct TextInput {
    pub text: String,
    pub mode: Mode,
    last: Option<(char, usize, u64)>,
    pub max: usize,
    /// keypad letters follow this language (accented letters)
    pub lang: Lang,
}

impl TextInput {
    pub fn new(mode: Mode) -> TextInput {
        TextInput { text: String::new(), mode, last: None, max: 2000, lang: Lang::En }
    }

    pub fn with(text: &str, mode: Mode) -> TextInput {
        let mut t = TextInput::new(mode);
        t.text = text.to_string();
        t
    }

    pub fn mode_label(&self) -> &'static str {
        match self.mode {
            Mode::Sentence => "Abc",
            Mode::Lower => "abc",
            Mode::Upper => "ABC",
            Mode::Num => "123",
        }
    }

    fn sentence_start(&self) -> bool {
        let t = self.text.trim_end_matches(' ');
        t.is_empty() || (self.text.ends_with(' ') && (t.ends_with('.') || t.ends_with('!') || t.ends_with('?')))
    }

    fn cased(&self, c: char) -> char {
        match self.mode {
            Mode::Upper => c.to_uppercase().next().unwrap_or(c),
            Mode::Sentence if self.sentence_start() => c.to_uppercase().next().unwrap_or(c),
            _ => c,
        }
    }

    fn tap(&mut self, key: char, set: &str, now: u64) {
        let chars: Vec<char> = set.chars().collect();
        if let Some((lk, idx, t)) = self.last {
            if lk == key && now.saturating_sub(t) < MULTITAP_MS {
                self.text.pop();
                let n = (idx + 1) % chars.len();
                let c = self.cased(chars[n]);
                self.text.push(c);
                self.last = Some((key, n, now));
                return;
            }
        }
        if util::char_len(&self.text) >= self.max {
            return;
        }
        let c = self.cased(chars[0]);
        self.text.push(c);
        self.last = Some((key, 0, now));
    }

    /// Returns true if the key was consumed.
    pub fn key(&mut self, k: Key, now: u64) -> bool {
        match k {
            Key::Num('#') => {
                self.mode = match self.mode {
                    Mode::Sentence => Mode::Lower,
                    Mode::Lower => Mode::Upper,
                    Mode::Upper => Mode::Num,
                    Mode::Num => Mode::Sentence,
                };
                self.last = None;
                true
            }
            Key::Num('*') => {
                self.tap('*', STAR, now);
                true
            }
            Key::Num(d) if d.is_ascii_digit() => {
                if self.mode == Mode::Num {
                    self.last = None;
                    if util::char_len(&self.text) < self.max {
                        self.text.push(d);
                    }
                } else {
                    self.tap(d, i18n::keypad(self.lang)[d as usize - '0' as usize], now);
                }
                true
            }
            Key::Char(c) => {
                self.last = None;
                if util::char_len(&self.text) < self.max {
                    self.text.push(c);
                }
                true
            }
            Key::Right => {
                self.last = None;
                true
            }
            Key::Back => {
                self.last = None;
                self.text.pop().is_some()
            }
            _ => false,
        }
    }

    /// Text with a cursor, wrapped.
    pub fn lines(&self, width: usize, now: u64) -> Vec<String> {
        let pending = self.last.is_some_and(|(_, _, t)| now.saturating_sub(t) < MULTITAP_MS);
        let mut s = self.text.clone();
        if !pending {
            s.push('_');
        }
        util::wrap(&s, width)
    }
}

#[cfg(test)]
mod t {
    use super::*;
    #[test]
    fn multitap() {
        let mut t = TextInput::new(Mode::Sentence);
        // "Hi" = 4,4 (h) then 4,4,4 (i)... h is 44, i is 444
        t.key(Key::Num('4'), 0);
        t.key(Key::Num('4'), 100);
        t.key(Key::Num('4'), 1500);
        t.key(Key::Num('4'), 1600);
        t.key(Key::Num('4'), 1700);
        t.key(Key::Num('0'), 3000);
        assert_eq!(t.text, "Hi ");
        t.key(Key::Num('#'), 4000);
        t.key(Key::Num('#'), 4000);
        t.key(Key::Num('#'), 4000);
        t.key(Key::Num('7'), 4100);
        assert_eq!(t.text, "Hi 7");
        assert!(t.key(Key::Back, 5000));
        t.key(Key::Char('!'), 5000);
        assert_eq!(t.text, "Hi !");
    }
}
