//! The preview line of the quick input: what the text means, or why it does not parse yet. Shared by the egui
//! popup and the native one.

use crate::i18n::Lang;
use crate::strings::{self, S};
use crate::{fmt, parse};

/// `(text, is_warning, understood)`.
pub fn line(text: &str, lang: Lang) -> (String, bool, bool) {
    let t = strings::TABLE.get(lang);
    let now = chrono::Local::now().naive_local();
    match parse::parse(text, now, lang) {
        Ok(p) => {
            let when = fmt::when_naive(p.fire_at, now, p.lang);
            let label = p.label_or_default();
            let len = fmt::dur(p.secs, lang);
            let mut line = if p.absolute { format!("{when}  ·  «{label}»  ·  {}", t.in_time.replace("{d}", &len)) } else { format!("{len}  ·  «{label}»  ·  {when}") };
            let mut warn = false;
            for n in &p.notes {
                match n {
                    parse::Note::ClampedMax => {
                        line.push_str(&format!("  ·  {}", t.hint_clamped));
                        warn = true;
                    }
                    parse::Note::IgnoredSecond(s) => {
                        line.push_str(&format!("  ·  {}", t.hint_ignored.replace("{t}", s)));
                        warn = true;
                    }
                    parse::Note::Tomorrow => {}
                }
            }
            (line, warn, true)
        }
        Err(parse::Error::Empty) | Err(parse::Error::NoTime) => (t.hint_no_time.to_string(), false, false),
        Err(parse::Error::NoUnit) => (t.hint_no_unit.to_string(), true, false),
        Err(parse::Error::Zero) => (t.hint_zero.to_string(), true, false),
        Err(parse::Error::BadClock) => (t.hint_bad_clock.to_string(), true, false),
    }
}

/// The hint for an error code the daemon returns.
pub fn error_text(code: &str, t: &'static S) -> &'static str {
    match code {
        "no_time" | "empty" => t.hint_no_time,
        "no_unit" => t.hint_no_unit,
        "zero" => t.hint_zero,
        "bad_clock" => t.hint_bad_clock,
        "too_many" => t.too_many,
        _ => t.add_failed,
    }
}
