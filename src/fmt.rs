//! Text for durations and times in the three languages.

use crate::i18n::Lang;
use crate::strings;
use chrono::{DateTime, Datelike, Local, NaiveDateTime, TimeZone, Timelike};

/// Seconds as "1 h 15 min": the two largest non-zero units.
pub fn dur(secs: i64, lang: Lang) -> String {
    let t = strings::TABLE.get(lang);
    let secs = secs.max(0);
    let (d, h, m, s) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60, secs % 60);
    let parts = [(d, t.days_short), (h, t.hours_short), (m, t.minutes_short), (s, t.seconds_short)];
    let out: Vec<String> = parts.iter().filter(|p| p.0 > 0).take(2).map(|p| format!("{} {}", p.0, p.1)).collect();
    if out.is_empty() { format!("0 {}", t.seconds_short) } else { out.join(" ") }
}

/// A live countdown: `04:59`, `1:02:03`, `2 d 3 h`.
pub fn countdown(secs: i64, lang: Lang) -> String {
    let secs = secs.max(0);
    if secs >= 86_400 {
        return dur(secs, lang);
    }
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

pub fn local(ms: i64) -> DateTime<Local> {
    Local.timestamp_millis_opt(ms).single().unwrap_or_else(Local::now)
}

/// `19:42`
pub fn hm(ms: i64) -> String {
    let t = local(ms);
    format!("{:02}:{:02}", t.hour(), t.minute())
}

fn weekday_short(lang: Lang, d: chrono::Weekday) -> &'static str {
    let i = d.num_days_from_monday() as usize;
    let en = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let ru = ["пн", "вт", "ср", "чт", "пт", "сб", "вс"];
    let uk = ["пн", "вт", "ср", "чт", "пт", "сб", "нд"];
    match lang {
        Lang::En => en[i],
        Lang::Ru => ru[i],
        Lang::Uk => uk[i],
    }
}

fn month_short(lang: Lang, m: u32) -> &'static str {
    let en = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let ru = ["янв", "фев", "мар", "апр", "мая", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"];
    let uk = ["січ", "лют", "бер", "кві", "тра", "чер", "лип", "сер", "вер", "жов", "лис", "гру"];
    let i = (m as usize).saturating_sub(1).min(11);
    match lang {
        Lang::En => en[i],
        Lang::Ru => ru[i],
        Lang::Uk => uk[i],
    }
}

/// "at 19:42", "tomorrow at 06:00", "Fri 27 Nov at 10:00", relative to `now`.
pub fn when(fire_ms: i64, now_ms: i64, lang: Lang) -> String {
    let t = strings::TABLE.get(lang);
    let (f, n) = (local(fire_ms), local(now_ms));
    let time = format!("{:02}:{:02}", f.hour(), f.minute());
    let days = (f.date_naive() - n.date_naive()).num_days();
    match days {
        0 => t.at_time.replace("{t}", &time),
        1 => t.tomorrow_at.replace("{t}", &time),
        _ => t.day_at.replace("{d}", &format!("{} {} {}", weekday_short(lang, f.weekday()), f.day(), month_short(lang, f.month()))).replace("{t}", &time),
    }
}

/// The same for a naive local time (the parser works with it).
pub fn when_naive(fire: NaiveDateTime, now: NaiveDateTime, lang: Lang) -> String {
    let to_ms = |n: NaiveDateTime| Local.from_local_datetime(&n).earliest().map(|d| d.timestamp_millis()).unwrap_or(0);
    when(to_ms(fire), to_ms(now), lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_show_two_units() {
        assert_eq!(dur(4500, Lang::En), "1 h 15 min");
        assert_eq!(dur(4500, Lang::Ru), "1 ч 15 мин");
        assert_eq!(dur(4500, Lang::Uk), "1 год 15 хв");
        assert_eq!(dur(90, Lang::En), "1 min 30 s");
        assert_eq!(dur(900, Lang::En), "15 min");
        assert_eq!(dur(86_400 * 2 + 3600 * 5 + 60, Lang::En), "2 d 5 h");
        assert_eq!(dur(0, Lang::En), "0 s");
    }

    #[test]
    fn countdown_is_a_clock() {
        assert_eq!(countdown(299, Lang::En), "04:59");
        assert_eq!(countdown(3723, Lang::En), "1:02:03");
        assert_eq!(countdown(-5, Lang::En), "00:00");
        assert_eq!(countdown(90_000, Lang::En), "1 d 1 h");
    }

    #[test]
    fn when_says_today_tomorrow_or_the_day() {
        let now = Local.with_ymd_and_hms(2026, 11, 25, 14, 0, 0).unwrap().timestamp_millis();
        let at = |d: u32, h: u32| Local.with_ymd_and_hms(2026, 11, d, h, 30, 0).unwrap().timestamp_millis();
        assert_eq!(when(at(25, 19), now, Lang::En), "at 19:30");
        assert_eq!(when(at(26, 6), now, Lang::En), "tomorrow at 06:30");
        assert_eq!(when(at(26, 6), now, Lang::Ru), "завтра в 06:30");
        assert_eq!(when(at(27, 10), now, Lang::En), "Fri 27 Nov at 10:30");
        assert_eq!(when(at(27, 10), now, Lang::Uk), "пт 27 лис о 10:30");
    }
}
