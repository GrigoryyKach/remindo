//! The vocabulary: units, number words, fillers, day names, in English, Russian and Ukrainian.
//!
//! Every function takes a *normalized* word (lowercase, `ё` folded, apostrophes removed; see `lex::normalize`).
//! Words that mix Latin and Cyrillic look-alikes (a Latin `m` inside `мин`) are folded both ways by [`variants`].

use crate::i18n::Lang;

/// Time units, ordered from the smallest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Unit {
    Sec,
    Min,
    Hour,
    Day,
    Week,
    Month,
}

impl Unit {
    pub fn secs(self) -> f64 {
        match self {
            Unit::Sec => 1.0,
            Unit::Min => 60.0,
            Unit::Hour => 3600.0,
            Unit::Day => 86_400.0,
            Unit::Week => 604_800.0,
            Unit::Month => 2_592_000.0,
        }
    }

    /// The next smaller unit (for `1h30`: the bare 30 is minutes).
    pub fn below(self) -> Option<Unit> {
        match self {
            Unit::Sec => None,
            Unit::Min => Some(Unit::Sec),
            Unit::Hour => Some(Unit::Min),
            Unit::Day => Some(Unit::Hour),
            Unit::Week => Some(Unit::Day),
            Unit::Month => Some(Unit::Day),
        }
    }
}

// ───────────────────────── look-alike folding ─────────────────────────

const LAT_TO_CYR: &[(char, char)] =
    &[('a', 'а'), ('c', 'с'), ('e', 'е'), ('o', 'о'), ('p', 'р'), ('x', 'х'), ('y', 'у'), ('k', 'к'), ('m', 'м'), ('t', 'т'), ('h', 'н'), ('i', 'і')];

fn is_cyr(c: char) -> bool {
    ('\u{0400}'..='\u{04FF}').contains(&c)
}

fn is_lat(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// The word itself, plus its all-Latin and all-Cyrillic readings when it mixes both scripts.
pub fn variants(w: &str) -> Vec<String> {
    let mut v = vec![w.to_string()];
    let has_cyr = w.chars().any(is_cyr);
    let has_lat = w.chars().any(is_lat);
    if has_cyr && has_lat {
        // to Cyrillic
        let to_cyr: Option<String> = w.chars().map(|c| if is_lat(c) { LAT_TO_CYR.iter().find(|p| p.0 == c).map(|p| p.1) } else { Some(c) }).collect();
        if let Some(s) = to_cyr {
            v.push(s);
        }
        // to Latin
        let to_lat: Option<String> = w.chars().map(|c| if is_cyr(c) { LAT_TO_CYR.iter().find(|p| p.1 == c).map(|p| p.0) } else { Some(c) }).collect();
        if let Some(s) = to_lat {
            v.push(s);
        }
    }
    v
}

// ───────────────────────── units ─────────────────────────

fn unit_exact(w: &str) -> Option<Unit> {
    Some(match w {
        // seconds
        "s" | "sec" | "secs" | "second" | "seconds" | "с" | "сек" | "секунда" | "секунды" | "секунду" | "секунд" | "секунди" | "секундочку" | "секундочки" => Unit::Sec,
        // minutes
        "m" | "min" | "mins" | "minute" | "minutes" | "м" | "мин" | "минута" | "минуты" | "минуту" | "минут" | "минутку" | "минутки" | "минуток" | "хв" | "хвилина"
        | "хвилини" | "хвилину" | "хвилин" | "хвилинку" | "хвилинки" => Unit::Min,
        // hours
        "h" | "hr" | "hrs" | "hour" | "hours" | "ч" | "час" | "часа" | "часов" | "часу" | "часы" | "часик" | "часок" | "год" | "година" | "години" | "годину" | "годин"
        | "годинку" | "годинки" => Unit::Hour,
        // days
        "d" | "day" | "days" | "д" | "дн" | "дня" | "дней" | "день" | "дни" | "дню" | "дні" | "днів" | "доба" | "доби" | "добу" | "діб" | "сутки" | "суток" | "сутку" => Unit::Day,
        // weeks
        "w" | "wk" | "wks" | "week" | "weeks" | "нед" | "неделя" | "неделю" | "недели" | "недель" | "тиж" | "тиждень" | "тижня" | "тижні" | "тижнів" | "тиждня" => Unit::Week,
        // months (only to say "too long")
        "month" | "months" | "мес" | "месяц" | "месяца" | "месяцев" | "місяць" | "місяця" | "місяці" | "місяців" => Unit::Month,
        _ => return None,
    })
}

/// The unit a word names. `cyr_context` allows the Latin `c` as seconds when the rest of the text is Cyrillic.
pub fn unit(w: &str, cyr_context: bool) -> Option<Unit> {
    for v in variants(w) {
        if let Some(u) = unit_exact(&v) {
            return Some(u);
        }
        if v.starts_with("секунд") {
            return Some(Unit::Sec);
        }
        if v.starts_with("минут") || v.starts_with("хвилин") {
            return Some(Unit::Min);
        }
        if v.starts_with("неделю") || v.starts_with("тижн") {
            return Some(Unit::Week);
        }
    }
    if cyr_context && w == "c" {
        return Some(Unit::Sec);
    }
    None
}

/// Units that can stand for "o'clock" after an hour number: `в 6 часов`, `о 6 годині`.
pub fn is_hour_marker(w: &str) -> bool {
    variants(w).iter().any(|v| matches!(v.as_str(), "час" | "часа" | "часов" | "годин" | "години" | "годині" | "годину" | "oclock" | "hours" | "hour" | "h" | "ч"))
}

// ───────────────────────── numbers ─────────────────────────

fn small_number(w: &str) -> Option<u32> {
    Some(match w {
        // English
        "zero" => 0,
        "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        "eleven" => 11,
        "twelve" => 12,
        "thirteen" => 13,
        "fourteen" => 14,
        "fifteen" => 15,
        "sixteen" => 16,
        "seventeen" => 17,
        "eighteen" => 18,
        "nineteen" => 19,
        "twenty" => 20,
        "thirty" => 30,
        "forty" => 40,
        "fifty" => 50,
        "sixty" => 60,
        "seventy" => 70,
        "eighty" => 80,
        "ninety" => 90,
        "hundred" => 100,
        // Russian and Ukrainian (apostrophes are already removed: п'ять -> пять)
        "ноль" | "нуль" => 0,
        "один" | "одна" | "одно" | "одну" | "одной" | "одного" | "одне" => 1,
        "два" | "две" | "двух" | "дві" | "двох" => 2,
        "три" | "трех" | "трьох" => 3,
        "четыре" | "четырех" | "чотири" | "чотирьох" => 4,
        "пять" | "пяти" | "пяти́" => 5,
        "шесть" | "шести" | "шість" | "шести́" => 6,
        "семь" | "семи" | "сім" => 7,
        "восемь" | "восьми" | "вісім" | "восьми́" => 8,
        "девять" | "девяти" | "девять́" => 9,
        "десять" | "десяти" => 10,
        "одиннадцать" | "одиннадцати" | "одинадцять" | "одинадцяти" => 11,
        "двенадцать" | "двенадцати" | "дванадцять" | "дванадцяти" => 12,
        "тринадцать" | "тринадцати" | "тринадцять" => 13,
        "четырнадцать" | "четырнадцати" | "чотирнадцять" => 14,
        "пятнадцать" | "пятнадцати" | "пятнадцять" | "пятнадцяти" => 15,
        "шестнадцать" | "шестнадцати" | "шістнадцять" => 16,
        "семнадцать" | "семнадцати" | "сімнадцять" => 17,
        "восемнадцать" | "восемнадцати" | "вісімнадцять" => 18,
        "девятнадцать" | "девятнадцати" | "девятнадцять" => 19,
        "двадцать" | "двадцати" | "двадцять" | "двадцяти" => 20,
        "тридцать" | "тридцати" | "тридцять" | "тридцяти" => 30,
        "сорок" | "сорока" => 40,
        "пятьдесят" | "пятидесяти" | "пятдесят" => 50,
        "шестьдесят" | "шестидесяти" | "шістдесят" => 60,
        "семьдесят" | "семидесяти" | "сімдесят" => 70,
        "восемьдесят" | "восьмидесяти" | "вісімдесят" => 80,
        "девяносто" | "девяноста" => 90,
        "сто" | "сту" => 100,
        _ => return None,
    })
}

/// A single number word (any of the three languages), with look-alike folding.
pub fn number_word(w: &str) -> Option<u32> {
    variants(w).iter().find_map(|v| small_number(v))
}

fn is_tens(n: u32) -> bool {
    matches!(n, 20 | 30 | 40 | 50 | 60 | 70 | 80 | 90)
}

/// Reads a number written in words from `words` (`двадцать пять`, `twenty five`, `сто двадцать`).
/// Returns the value and how many words it used.
pub fn number_words(words: &[&str]) -> Option<(u32, usize)> {
    let first = number_word(words.first()?)?;
    if first == 100 && words.first().is_some_and(|w| matches!(*w, "hundred")) {
        // "hundred" alone is not a quantity here ("a hundred" is handled by the caller as an article + hundred)
        return None;
    }
    let mut value = first;
    let mut used = 1;
    if first == 100 {
        if let Some(n) = words.get(1).and_then(|w| number_word(w)) {
            if n > 0 && n < 100 {
                value += n;
                used = 2;
                if is_tens(n) {
                    if let Some(u) = words.get(2).and_then(|w| number_word(w)) {
                        if (1..10).contains(&u) {
                            value += u;
                            used = 3;
                        }
                    }
                }
            }
        }
        return Some((value, used));
    }
    if is_tens(first) {
        if let Some(u) = words.get(1).and_then(|w| number_word(w)) {
            if (1..10).contains(&u) {
                value += u;
                used = 2;
            }
        }
    }
    Some((value, used))
}

/// "a couple", "a few", "пару", "несколько", "кілька": vague quantities.
pub fn vague_quantity(w: &str) -> Option<u32> {
    match w {
        "couple" | "пару" | "пара" | "пары" | "парочку" => Some(2),
        "few" | "несколько" | "кілька" | "декілька" | "пару-тройку" => Some(3),
        _ => None,
    }
}

pub fn is_article(w: &str) -> bool {
    matches!(w, "a" | "an")
}

// ───────────────────────── halves and quarters ─────────────────────────

/// One-word halves: `полчаса`, `півгодини`, `полминуты`: the unit and the amount.
pub fn half_word(w: &str) -> Option<(Unit, f64)> {
    Some(match w {
        "полчаса" | "полчаса́" | "півгодини" | "півгодину" | "полчасика" => (Unit::Hour, 0.5),
        "полминуты" | "полминуту" | "півхвилини" | "півхвилину" => (Unit::Min, 0.5),
        "полдня" | "півдня" | "полудня" => (Unit::Day, 0.5),
        "полсекунды" | "півсекунди" => (Unit::Sec, 0.5),
        _ => return None,
    })
}

/// `полтора` / `полторы` / `півтори` / `півтора`: one and a half of the unit that follows.
pub fn one_and_half(w: &str) -> bool {
    matches!(w, "полтора" | "полторы" | "півтори" | "півтора" | "полтори")
}

/// The first word of `пол часа` / `пів години` (written apart).
pub fn half_prefix(w: &str) -> bool {
    matches!(w, "пол" | "пів" | "пiв")
}

pub fn is_half(w: &str) -> bool {
    matches!(w, "half")
}

pub fn is_quarter_word(w: &str) -> bool {
    matches!(w, "четверть" | "чверть")
}

/// `четверти` / `четверть` / `чверті`: a quarter, preceded by a number (`три четверти часа`).
pub fn is_quarters(w: &str) -> bool {
    matches!(w, "четверти" | "четвертей" | "чверті" | "чвертей" | "quarters" | "quarter")
}

pub fn is_with_half(w: &str) -> bool {
    matches!(w, "половиной" | "половиною")
}

// ───────────────────────── fillers ─────────────────────────

/// Words that announce a duration and are part of the time phrase: `in 5 minutes`, `через 5 минут`.
pub fn strong_dur_filler(w: &str, lang_uk: bool) -> bool {
    match w {
        "in" | "after" | "within" | "through" | "через" | "спустя" | "після" | "протягом" => true,
        "за" => lang_uk,
        _ => false,
    }
}

/// Weaker words that go with a duration (`for 10 minutes`, `на 10 минут`): consumed with it, but they do not make it certain.
pub fn weak_dur_filler(w: &str) -> bool {
    matches!(w, "for" | "на" | "за")
}

pub fn approx_word(w: &str) -> bool {
    matches!(w, "about" | "around" | "approximately" | "примерно" | "около" | "приблизно" | "десь" | "близько")
}

/// "at" in the three languages (and "by" / "до" / "к").
pub fn at_filler(w: &str) -> bool {
    matches!(w, "at" | "by" | "until" | "till" | "в" | "во" | "о" | "об" | "у" | "к" | "до" | "ко")
}

/// A preposition that may be left dangling next to a removed time phrase.
pub fn dangling_filler(w: &str) -> bool {
    matches!(w, "in" | "after" | "for" | "within" | "at" | "by" | "on" | "until" | "till" | "через" | "спустя" | "за" | "на" | "в" | "во" | "о" | "у" | "к" | "до" | "після" | "протягом")
}

pub fn from_now_suffix(words: &[&str]) -> usize {
    match words {
        ["from", "now", ..] => 2,
        ["later", ..] | ["позже", ..] | ["спустя", ..] | ["пізніше", ..] | ["потім", ..] => 1,
        _ => 0,
    }
}

pub fn and_connector(w: &str) -> bool {
    matches!(w, "and" | "и" | "і" | "й" | "та" | "plus")
}

// ───────────────────────── days ─────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DayWord {
    Today,
    Tomorrow,
    AfterTomorrow,
    /// 0 = Monday ... 6 = Sunday
    Weekday(u32),
}

pub fn day_word(w: &str) -> Option<DayWord> {
    Some(match w {
        "today" | "сегодня" | "сьогодні" => DayWord::Today,
        "tomorrow" | "tmrw" | "tomorow" | "завтра" => DayWord::Tomorrow,
        "послезавтра" | "післязавтра" => DayWord::AfterTomorrow,
        "monday" | "mon" | "понедельник" | "понеділок" | "пн" | "понедельника" => DayWord::Weekday(0),
        "tuesday" | "tue" | "tues" | "вторник" | "вівторок" | "вт" => DayWord::Weekday(1),
        "wednesday" | "wed" | "среда" | "среду" | "середа" | "середу" | "ср" => DayWord::Weekday(2),
        "thursday" | "thu" | "thur" | "thurs" | "четверг" | "четвер" | "чт" => DayWord::Weekday(3),
        "friday" | "fri" | "пятница" | "пятницу" | "пятницю" | "пятниця" | "пт" => DayWord::Weekday(4),
        "saturday" | "sat" | "суббота" | "субботу" | "субота" | "суботу" | "сб" => DayWord::Weekday(5),
        "sunday" | "sun" | "воскресенье" | "неділя" | "неділю" | "вс" | "нд" => DayWord::Weekday(6),
        _ => return None,
    })
}

/// Abbreviations (`mon`, `пн`, `вс`) are too common as plain words to count without an "at" and a time.
pub fn is_short_weekday(w: &str) -> bool {
    matches!(w, "mon" | "tue" | "tues" | "wed" | "thu" | "thur" | "thurs" | "fri" | "sat" | "sun" | "пн" | "вт" | "ср" | "чт" | "пт" | "сб" | "вс" | "нд" | "sunday")
}

// ───────────────────────── day parts ─────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DayPart {
    Morning,
    Day,
    Evening,
    Night,
}

pub fn day_part(w: &str) -> Option<DayPart> {
    Some(match w {
        "утра" | "утром" | "ранку" | "зранку" | "ранок" | "morning" => DayPart::Morning,
        "дня" | "днем" | "вдень" | "afternoon" => DayPart::Day,
        "вечера" | "вечером" | "вечора" | "ввечері" | "вечір" | "evening" => DayPart::Evening,
        "ночи" | "ночью" | "ночі" | "вночі" | "night" => DayPart::Night,
        _ => return None,
    })
}

/// Applies the day part to an hour written on a 12-hour clock.
pub fn apply_day_part(h: u32, part: DayPart) -> u32 {
    match part {
        DayPart::Morning => {
            if h == 12 { 0 } else { h }
        }
        DayPart::Day => {
            if (1..=6).contains(&h) { h + 12 } else { h }
        }
        DayPart::Evening => {
            if h < 12 { h + 12 } else { h }
        }
        DayPart::Night => {
            if h == 12 {
                0
            } else if (9..12).contains(&h) {
                h + 12
            } else {
                h
            }
        }
    }
}

pub fn is_am(w: &str) -> bool {
    w == "am"
}

pub fn is_pm(w: &str) -> bool {
    w == "pm"
}

/// `noon` / `midnight` and friends, as an hour.
pub fn named_time(w: &str) -> Option<u32> {
    match w {
        "noon" | "полдень" | "полудень" | "опівдні" | "південь" | "півдні" => Some(12),
        "midnight" | "полночь" | "північ" | "півночі" | "опівночі" | "полуночи" => Some(0),
        _ => None,
    }
}

// ───────────────────────── "remind me" ─────────────────────────

/// Leading phrases to drop from the label, longest first.
pub const REMIND_PREFIXES: &[&[&str]] = &[
    &["remind", "me", "to"],
    &["remind", "me", "about"],
    &["remind", "me", "that"],
    &["remind", "me", "of"],
    &["remind", "me"],
    &["reminder", "to"],
    &["reminder", "about"],
    &["reminder"],
    &["please", "remind", "me", "to"],
    &["please", "remind", "me"],
    &["напомни", "мне", "что"],
    &["напомни", "мне", "чтобы"],
    &["напомни", "мне", "про"],
    &["напомни", "мне"],
    &["напомни", "что"],
    &["напомни", "чтобы"],
    &["напомни", "про"],
    &["напомните", "мне"],
    &["напомните"],
    &["напомнить", "мне"],
    &["напомнить"],
    &["напомни"],
    &["напоминание"],
    &["нагадай", "мені", "що"],
    &["нагадай", "мені", "щоб"],
    &["нагадай", "мені", "про"],
    &["нагадай", "мені"],
    &["нагадай", "що"],
    &["нагадай", "щоб"],
    &["нагадай", "про"],
    &["нагадайте", "мені"],
    &["нагадайте"],
    &["нагадати", "мені"],
    &["нагадати"],
    &["нагадай"],
    &["нагадування"],
];

// ───────────────────────── language evidence ─────────────────────────

/// Words that only one of the three languages uses. Returns (en, ru, uk) evidence points.
pub fn language_hint(norm_word: &str) -> (u32, u32, u32) {
    let w = norm_word;
    // Letters that exist only in Ukrainian or only in Russian.
    let uk_letters = w.chars().any(|c| matches!(c, 'і' | 'ї' | 'є' | 'ґ'));
    let ru_letters = w.chars().any(|c| matches!(c, 'ы' | 'э' | 'ъ'));
    if uk_letters {
        return (0, 0, 2);
    }
    if ru_letters {
        return (0, 2, 0);
    }
    if w.chars().any(is_cyr) {
        if matches!(w, "хв" | "хвилин" | "хвилина" | "хвилини" | "хвилину" | "годин" | "години" | "годину" | "година" | "діб" | "доба" | "доби" | "нагадай" | "нагадати" | "кілька" | "півтори" | "тиждень" | "тижня" | "сьогодні" | "післязавтра" | "чверть" | "півгодини" | "щоб" | "що") {
            return (0, 0, 2);
        }
        if matches!(w, "минут" | "минута" | "минуты" | "минуту" | "час" | "часа" | "часов" | "напомни" | "напомнить" | "сегодня" | "послезавтра" | "полтора" | "полторы" | "спустя" | "через" | "чтобы" | "неделя" | "неделю" | "полчаса" | "несколько" | "утра" | "вечера" | "дня" | "дней" | "сутки" | "пару" | "мин") {
            // "через" and "четверть" are shared in part, but they still say "not English".
            return if matches!(w, "через" | "мин") { (0, 1, 1) } else { (0, 2, 0) };
        }
        return (0, 1, 1);
    }
    if w.chars().any(|c| c.is_ascii_alphabetic()) && w.len() > 2 {
        return (1, 0, 0);
    }
    (0, 0, 0)
}

/// Guesses the language of the text from its words; `fallback` decides when the text does not say.
pub fn detect_language(words: &[&str], fallback: Lang) -> Lang {
    let (mut en, mut ru, mut uk) = (0u32, 0u32, 0u32);
    let mut cyr = false;
    for w in words {
        let (a, b, c) = language_hint(w);
        en += a;
        ru += b;
        uk += c;
        if w.chars().any(is_cyr) {
            cyr = true;
        }
    }
    if uk > ru && uk > 0 {
        Lang::Uk
    } else if ru > uk && ru > 0 {
        Lang::Ru
    } else if cyr {
        match fallback {
            Lang::En => Lang::Ru,
            l => l,
        }
    } else if en > 0 {
        Lang::En
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_in_three_languages() {
        for (w, u) in [
            ("sec", Unit::Sec),
            ("секунду", Unit::Sec),
            ("секунди", Unit::Sec),
            ("min", Unit::Min),
            ("минут", Unit::Min),
            ("хв", Unit::Min),
            ("хвилини", Unit::Min),
            ("hrs", Unit::Hour),
            ("часа", Unit::Hour),
            ("годин", Unit::Hour),
            ("год", Unit::Hour),
            ("дней", Unit::Day),
            ("доба", Unit::Day),
            ("днів", Unit::Day),
            ("тижні", Unit::Week),
        ] {
            assert_eq!(unit(w, false), Some(u), "{w}");
        }
        assert_eq!(unit("годинник", false), None);
        assert_eq!(unit("часть", false), None);
        assert_eq!(unit("мінімум", false), None);
    }

    #[test]
    fn look_alikes() {
        // Cyrillic м in a Latin word and Latin letters in a Cyrillic word.
        assert_eq!(unit("мin", false), Some(Unit::Min));
        assert_eq!(unit("минyт", false), Some(Unit::Min));
        assert_eq!(unit("m", false), Some(Unit::Min));
        assert_eq!(unit("м", false), Some(Unit::Min));
        assert_eq!(unit("c", false), None);
        assert_eq!(unit("c", true), Some(Unit::Sec));
    }

    #[test]
    fn number_words_combine() {
        assert_eq!(number_words(&["twenty", "five", "minutes"]), Some((25, 2)));
        assert_eq!(number_words(&["двадцать", "пять", "минут"]), Some((25, 2)));
        assert_eq!(number_words(&["сорок"]), Some((40, 1)));
        assert_eq!(number_words(&["fifteen"]), Some((15, 1)));
        assert_eq!(number_words(&["сто", "двадцать"]), Some((120, 2)));
        assert_eq!(number_words(&["плита"]), None);
        assert_eq!(number_words(&["пять", "двадцать"]), Some((5, 1)));
    }

    #[test]
    fn day_parts_shift_the_hour() {
        assert_eq!(apply_day_part(6, DayPart::Evening), 18);
        assert_eq!(apply_day_part(9, DayPart::Morning), 9);
        assert_eq!(apply_day_part(12, DayPart::Morning), 0);
        assert_eq!(apply_day_part(3, DayPart::Day), 15);
        assert_eq!(apply_day_part(12, DayPart::Day), 12);
        assert_eq!(apply_day_part(11, DayPart::Night), 23);
        assert_eq!(apply_day_part(2, DayPart::Night), 2);
    }

    #[test]
    fn language_detection() {
        assert_eq!(detect_language(&["через", "пять", "минут"], Lang::En), Lang::Ru);
        assert_eq!(detect_language(&["через", "п'ять", "хвилин"], Lang::En), Lang::Uk);
        assert_eq!(detect_language(&["in", "five", "minutes"], Lang::Ru), Lang::En);
        assert_eq!(detect_language(&["15"], Lang::Uk), Lang::Uk);
        assert_eq!(detect_language(&["плита"], Lang::En), Lang::Ru);
    }
}
