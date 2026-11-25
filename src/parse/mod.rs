//! The timer parser: free text in English, Russian or Ukrainian (mixed, any case) in, a fire time and a label out.
//!
//! Deterministic and rule based: no network, no model. The pipeline:
//!
//! 1. [`lex`] splits the text into numbers, words and punctuation with their positions in the original string;
//! 2. [`phrase`] finds every *time phrase* (`через 15 минут`, `1ч15м`, `at 18:30`, `завтра в 9`) and ranks it by how
//!    sure it is (an explicit "in" / "через" / "at" beats a bare `2 м`);
//! 3. the best phrase is cut out of the original text, and what is left, trimmed of fillers and separators
//!    (`напомни`, `remind me to`, `-`, `,`), is the label, in its original case and wording;
//! 4. the phrase becomes a fire time relative to `now`, with the rules for "already past today means tomorrow"
//!    and the 7 day limit.

mod lex;
mod phrase;
#[cfg(test)]
mod tests;
mod words;

use crate::i18n::Lang;
use chrono::{Datelike, Duration, NaiveDateTime, NaiveTime, Timelike};
use phrase::{Cand, Phrase};
use words::DayWord;

/// The longest timer: seven days.
pub const MAX_SECS: i64 = 7 * 86_400;

/// Something the preview should mention next to a successful parse.
#[derive(Clone, Debug, PartialEq)]
pub enum Note {
    /// The clock time was already past today, so the timer is set for tomorrow.
    Tomorrow,
    /// The time was longer than seven days and was cut to seven days.
    ClampedMax,
    /// A second time phrase was found; the first one was used and this text was kept in the label.
    IgnoredSecond(String),
}

/// Why nothing could be set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Nothing typed yet.
    Empty,
    /// No time in the text.
    NoTime,
    /// "in 5" without a unit.
    NoUnit,
    /// A zero length (`0 min`).
    Zero,
    /// A clock time that does not exist (`at 25:70`).
    BadClock,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    /// Seconds from `now` until the timer fires.
    pub secs: i64,
    pub fire_at: NaiveDateTime,
    /// The text named a clock time (`at 18:30`), not a length.
    pub absolute: bool,
    /// The fire time is on the next day (for the preview: "tomorrow at 06:00").
    pub next_day: bool,
    /// The label as typed, trimmed. Empty when there is none (see [`default_label`]).
    pub label: String,
    /// The language the text was written in (decides the default label).
    pub lang: Lang,
    pub notes: Vec<Note>,
}

/// "Timer" / «Таймер» for an empty label.
pub fn default_label(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Timer",
        Lang::Ru | Lang::Uk => "Таймер",
    }
}

impl Parsed {
    /// The label to show and store: the typed one, or the default.
    pub fn label_or_default(&self) -> String {
        if self.label.is_empty() { default_label(self.lang).to_string() } else { self.label.clone() }
    }
}

/// Parses `input`. `now` is the local wall clock; `ui_lang` decides the language when the text does not say.
pub fn parse(input: &str, now: NaiveDateTime, ui_lang: Lang) -> Result<Parsed, Error> {
    if input.trim().is_empty() {
        return Err(Error::Empty);
    }
    let toks = lex::tokenize(input);
    let norm_words: Vec<&str> = toks.iter().filter(|t| t.is_word()).map(|t| t.norm.as_str()).collect();
    let lang = words::detect_language(&norm_words, ui_lang);
    let found = phrase::find(&toks, lang == Lang::Uk, input.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)));
    let Some(best) = found.best else {
        return Err(if found.bad_clock {
            Error::BadClock
        } else if found.no_unit {
            Error::NoUnit
        } else {
            Error::NoTime
        });
    };

    // The time itself.
    let mut notes = Vec::new();
    let (mut fire_at, absolute, mut next_day) = match &best.phrase {
        Phrase::Dur { secs } => {
            let whole = (secs.round() as i64).min(MAX_SECS + 1);
            if whole < 1 {
                return Err(Error::Zero);
            }
            let dt = now + Duration::seconds(whole);
            (dt, false, dt.date() != now.date())
        }
        Phrase::At { h, m, day } => {
            let (dt, rolled) = resolve_clock(now, *h, *m, *day);
            if rolled {
                notes.push(Note::Tomorrow);
            }
            (dt, true, dt.date() != now.date())
        }
    };
    if (fire_at - now).num_seconds() > MAX_SECS {
        fire_at = now + Duration::seconds(MAX_SECS);
        notes.push(Note::ClampedMax);
        next_day = fire_at.date() != now.date();
    }
    let secs = (fire_at - now).num_seconds();

    // The label: the text without the phrase.
    let label = build_label(input, &toks, &best, lang);
    if let Some(second) = found.second {
        let text = input[toks[second.start].start..toks[second.end - 1].end].to_string();
        notes.push(Note::IgnoredSecond(text));
    }
    Ok(Parsed { secs, fire_at, absolute, next_day, label, lang, notes })
}

/// The next occurrence of a clock time. Returns the moment and whether it was moved to a later day because the
/// time had passed.
fn resolve_clock(now: NaiveDateTime, h: u32, m: u32, day: Option<DayWord>) -> (NaiveDateTime, bool) {
    let (h, extra_day) = if h >= 24 { (0, 1) } else { (h, 0) };
    let time = NaiveTime::from_hms_opt(h, m, 0).unwrap_or(NaiveTime::MIN);
    let today = now.date();
    let at = |d: chrono::NaiveDate| d.and_time(time) + Duration::days(extra_day);
    match day {
        None | Some(DayWord::Today) => {
            let dt = at(today);
            if dt <= now { (dt + Duration::days(1), true) } else { (dt, false) }
        }
        Some(DayWord::Tomorrow) => (at(today + Duration::days(1)), false),
        Some(DayWord::AfterTomorrow) => (at(today + Duration::days(2)), false),
        Some(DayWord::Weekday(w)) => {
            let delta = (w as i64 - today.weekday().num_days_from_monday() as i64).rem_euclid(7);
            let dt = at(today + Duration::days(delta));
            if dt <= now { (dt + Duration::days(7), false) } else { (dt, false) }
        }
    }
}

// ───────────────────────── label ─────────────────────────

fn is_separator(c: char) -> bool {
    c.is_whitespace() || matches!(c, '-' | '—' | '–' | ':' | ',' | ';' | '|' | '/' | '·' | '•' | '.')
}

fn trim_seps(s: &str) -> &str {
    s.trim_matches(is_separator)
}

/// Words the label may begin with after a "remind me" that are not part of the message.
fn is_label_lead(w: &str) -> bool {
    matches!(w, "to" | "that" | "about" | "что" | "чтобы" | "щоб" | "що")
}

fn build_label(input: &str, toks: &[lex::Tok], best: &Cand, _lang: Lang) -> String {
    let mut start = toks[best.start].start;
    let end = toks[best.end - 1].end;
    // A preposition left right before the phrase ("set a timer for 10 min") goes with it.
    if best.start > 0 {
        let p = &toks[best.start - 1];
        if p.is_word() && p.end <= start && words::dangling_filler(&p.norm) && !best.consumed_filler {
            start = p.start;
        }
    }
    let mut left = input[..start].trim_end_matches(is_separator);
    let mut right = input[end..].trim_start_matches(is_separator);
    // The phrase sat alone inside brackets or quotes: "(через 5 минут) чай" -> "чай".
    while let (Some(open), Some(close)) = (left.chars().last(), right.chars().next()) {
        let pair = matches!((open, close), ('(', ')') | ('[', ']') | ('{', '}') | ('«', '»') | ('“', '”') | ('"', '"') | ('\'', '\''));
        if !pair {
            break;
        }
        left = left[..left.len() - open.len_utf8()].trim_end_matches(is_separator);
        right = right[close.len_utf8()..].trim_start_matches(is_separator);
    }
    let at_start = left.trim().is_empty();
    let mut label = match (left.is_empty(), right.is_empty()) {
        (true, true) => String::new(),
        (true, false) => right.to_string(),
        (false, true) => left.to_string(),
        (false, false) if right.starts_with(['!', '?', ')', ']', '»', '”', '…']) => format!("{left}{right}"),
        (false, false) => format!("{left} {right}"),
    };
    label = collapse_spaces(trim_seps(&label));
    if !label.chars().any(|c| c.is_alphanumeric()) {
        label.clear();
    }

    // "remind me to ...", "напомни ...": cut from the start; the same words at the end ("..., напомни").
    let mut stripped_remind = false;
    loop {
        let before = label.clone();
        label = strip_remind_prefix(&label, &mut stripped_remind);
        label = strip_remind_suffix(&label);
        label = collapse_spaces(trim_seps(&label));
        if label == before {
            break;
        }
    }
    if at_start || stripped_remind {
        // "in 5 minutes to call mom" -> "call mom"
        let first: String = label.split_whitespace().next().unwrap_or("").to_string();
        if is_label_lead(&lex::normalize(&first)) && label.split_whitespace().count() > 1 {
            label = label[first.len()..].trim_start().to_string();
        }
    }
    collapse_spaces(trim_seps(&label))
}

fn collapse_spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_remind_prefix(label: &str, stripped: &mut bool) -> String {
    let toks = lex::tokenize(label);
    let words: Vec<&str> = toks.iter().map(|t| if t.is_word() { t.norm.as_str() } else { "\u{0}" }).collect();
    for pre in words::REMIND_PREFIXES {
        if words.len() >= pre.len() && words[..pre.len()] == **pre {
            // Leave a label that is nothing but the remind words alone as empty.
            *stripped = true;
            let cut = toks[pre.len() - 1].end;
            return trim_seps(&label[cut..]).to_string();
        }
    }
    label.to_string()
}

fn strip_remind_suffix(label: &str) -> String {
    let toks = lex::tokenize(label);
    let words: Vec<&str> = toks.iter().map(|t| if t.is_word() { t.norm.as_str() } else { "\u{0}" }).collect();
    const SUFFIXES: &[&[&str]] = &[&["remind", "me"], &["напомни", "мне"], &["напомни"], &["нагадай", "мені"], &["нагадай"], &["please"], &["пожалуйста"], &["будь", "ласка"], &["будь-ласка"]];
    for suf in SUFFIXES {
        if words.len() >= suf.len() && words[words.len() - suf.len()..] == **suf {
            let cut = toks[words.len() - suf.len()].start;
            return label[..cut].trim_end_matches(is_separator).to_string();
        }
    }
    label.to_string()
}

#[allow(dead_code)]
fn time_of(dt: NaiveDateTime) -> (u32, u32) {
    (dt.hour(), dt.minute())
}
