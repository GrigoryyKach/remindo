//! The tokenizer. Splits the input into numbers, words and punctuation and remembers where every token sits in
//! the original text, so the label can be cut out of it later without touching its case or wording.

/// What a token is.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Digits only, e.g. `15`.
    Int(u32),
    /// `2.5` or `2,5` (digits on both sides of the separator, no spaces).
    Dec(f64),
    /// `1:15` or `18:30` or `0:01:30`: hours, minutes and optional seconds. The flag says the hour had a leading zero.
    Clock { h: u32, m: u32, s: Option<u32>, zero: bool },
    /// Letters (apostrophes inside are kept, e.g. `п'ять`).
    Word,
    /// Anything else, one character.
    Punct(char),
}

#[derive(Clone, Debug)]
pub struct Tok {
    pub kind: Kind,
    /// Byte range in the original text.
    pub start: usize,
    pub end: usize,
    /// The original text of the token.
    pub text: String,
    /// Lowercased, `ё` folded to `е`, apostrophes removed. Empty for non-words.
    pub norm: String,
    /// No whitespace between this token and the previous one.
    pub glued: bool,
}

impl Tok {
    pub fn is_word(&self) -> bool {
        self.kind == Kind::Word
    }
    pub fn word(&self) -> &str {
        if self.is_word() { &self.norm } else { "" }
    }
    pub fn is_punct(&self, c: char) -> bool {
        self.kind == Kind::Punct(c)
    }
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '’' | 'ʼ' | '`' | '´' | '′' | '‘')
}

fn is_letter(c: char) -> bool {
    c.is_alphabetic()
}

/// Lowercase, `ё` -> `е`, drop apostrophes.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if is_apostrophe(c) {
            continue;
        }
        for l in c.to_lowercase() {
            out.push(if l == 'ё' { 'е' } else { l });
        }
    }
    out
}

pub fn tokenize(input: &str) -> Vec<Tok> {
    let chars: Vec<(usize, char)> = input.char_indices().collect();
    let n = chars.len();
    let byte_at = |i: usize| if i < n { chars[i].0 } else { input.len() };
    let mut toks = Vec::new();
    let mut i = 0;
    let mut prev_end: Option<usize> = None;
    while i < n {
        let (b, c) = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let glued = prev_end == Some(b);
        let mut j = i + 1;
        let kind;
        if c.is_ascii_digit() {
            while j < n && chars[j].1.is_ascii_digit() {
                j += 1;
            }
            let int_text: String = chars[i..j].iter().map(|x| x.1).collect();
            // `1:15` / `18:30` / `0:01:30`
            let mut clock = None;
            if j + 1 < n && chars[j].1 == ':' && chars[j + 1].1.is_ascii_digit() {
                let mut k = j + 1;
                while k < n && chars[k].1.is_ascii_digit() {
                    k += 1;
                }
                let min_text: String = chars[j + 1..k].iter().map(|x| x.1).collect();
                let mut end = k;
                let mut sec = None;
                if k + 1 < n && chars[k].1 == ':' && chars[k + 1].1.is_ascii_digit() {
                    let mut e = k + 1;
                    while e < n && chars[e].1.is_ascii_digit() {
                        e += 1;
                    }
                    let st: String = chars[k + 1..e].iter().map(|x| x.1).collect();
                    sec = st.parse::<u32>().ok().filter(|_| st.len() <= 2);
                    if sec.is_some() {
                        end = e;
                    }
                }
                if min_text.len() <= 2 && int_text.len() <= 2 {
                    if let (Ok(h), Ok(m)) = (int_text.parse::<u32>(), min_text.parse::<u32>()) {
                        clock = Some((end, Kind::Clock { h, m, s: sec, zero: int_text.len() == 2 && int_text.starts_with('0') }));
                    }
                }
            }
            if let Some((end, k)) = clock {
                j = end;
                kind = k;
            } else if j + 1 < n && matches!(chars[j].1, '.' | ',') && chars[j + 1].1.is_ascii_digit() {
                let mut k = j + 1;
                while k < n && chars[k].1.is_ascii_digit() {
                    k += 1;
                }
                // A second separator right after (a date like 12.05.2025, a list 1,2,3) is not a decimal.
                let more = k + 1 < n && matches!(chars[k].1, '.' | ',') && chars[k + 1].1.is_ascii_digit();
                let text: String = chars[i..k].iter().map(|x| if matches!(x.1, ',') { '.' } else { x.1 }).collect();
                match (more, text.parse::<f64>()) {
                    (false, Ok(v)) => {
                        j = k;
                        kind = Kind::Dec(v);
                    }
                    _ => kind = int_kind(&int_text),
                }
            } else {
                kind = int_kind(&int_text);
            }
        } else if is_letter(c) {
            while j < n {
                let cj = chars[j].1;
                if is_letter(cj) {
                    j += 1;
                } else if is_apostrophe(cj) && j + 1 < n && is_letter(chars[j + 1].1) && j > i {
                    j += 1;
                } else {
                    break;
                }
            }
            // `p.m.` / `a.m.`
            let w: String = chars[i..j].iter().map(|x| x.1).collect();
            let lw = w.to_lowercase();
            if (lw == "a" || lw == "p") && j + 1 < n && chars[j].1 == '.' && chars[j + 1].1.eq_ignore_ascii_case(&'m') {
                j += 2;
                if j < n && chars[j].1 == '.' {
                    j += 1;
                }
            }
            kind = Kind::Word;
        } else {
            kind = Kind::Punct(c);
        }
        let end = byte_at(j);
        let text = input[b..end].to_string();
        let norm = if kind == Kind::Word { normalize(&text.replace('.', "")) } else { String::new() };
        toks.push(Tok { kind, start: b, end, text, norm, glued });
        prev_end = Some(end);
        i = j;
    }
    toks
}

fn int_kind(s: &str) -> Kind {
    match s.parse::<u32>() {
        Ok(n) => Kind::Int(n),
        // Absurdly long numbers are kept as a huge value so they never look like a time.
        Err(_) => Kind::Int(u32::MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(s: &str) -> Vec<Kind> {
        tokenize(s).into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn splits_numbers_and_units() {
        let t = tokenize("1ч15м - плита");
        assert_eq!(t.len(), 6);
        assert_eq!(t[0].kind, Kind::Int(1));
        assert_eq!(t[1].norm, "ч");
        assert!(t[1].glued);
        assert_eq!(t[2].kind, Kind::Int(15));
        assert!(t[4].is_punct('-'));
        assert_eq!(t[5].text, "плита");
    }

    #[test]
    fn decimals_clocks_and_dates() {
        assert_eq!(kinds("2.5h")[0], Kind::Dec(2.5));
        assert_eq!(kinds("2,5 ч")[0], Kind::Dec(2.5));
        assert_eq!(kinds("1:15")[0], Kind::Clock { h: 1, m: 15, s: None, zero: false });
        assert_eq!(kinds("18:30")[0], Kind::Clock { h: 18, m: 30, s: None, zero: false });
        assert_eq!(kinds("0:01:30")[0], Kind::Clock { h: 0, m: 1, s: Some(30), zero: false });
        // A list is not a decimal.
        assert_eq!(kinds("1,2,3")[0], Kind::Int(1));
        // A comma followed by a space is a separator.
        assert_eq!(kinds("15, плита")[0], Kind::Int(15));
    }

    #[test]
    fn apostrophes_stay_inside_words() {
        let t = tokenize("п'ять хвилин");
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].norm, "пять");
        let t = tokenize("пʼятнадцять");
        assert_eq!(t[0].norm, "пятнадцять");
    }

    #[test]
    fn am_pm_with_dots() {
        let t = tokenize("6 p.m. call");
        assert_eq!(t[1].norm, "pm");
        assert_eq!(t[2].norm, "call");
        let t = tokenize("6pm");
        assert_eq!(t[1].norm, "pm");
    }

    #[test]
    fn spans_point_into_the_original() {
        let s = "Выключить ПЛИТУ через 15 минут";
        for t in tokenize(s) {
            assert_eq!(&s[t.start..t.end], t.text);
        }
    }
}
