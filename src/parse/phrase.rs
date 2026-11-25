//! Finds time phrases in a token stream.
//!
//! A phrase is either a *duration* (`через 1 час 15 минут`, `1ч15м`, `полчаса`, `in an hour and a half`) or a
//! *clock time* (`at 18:30`, `в 6 вечера`, `завтра в 9`, `6pm`). Every phrase gets a rank: the more explicit
//! it is, the higher, so `купить 2 м ткани через 10 минут` takes the "через" phrase and leaves `2 м` in the label.
//!
//! Ranks: 1 = a bare hour after "at" (`в 5`), 2 = a bare duration or clock (`15м`, `18:30`, `6pm`),
//! 3 = introduced by a word (`через`, `in`, `at`, `в`) or carrying a day (`завтра в 9`).

use super::lex::{Kind, Tok};
use super::words::{self, DayWord, Unit};

#[derive(Clone, Debug, PartialEq)]
pub enum Phrase {
    Dur { secs: f64 },
    At { h: u32, m: u32, day: Option<DayWord> },
}

#[derive(Clone, Debug)]
pub struct Cand {
    /// Token range `[start, end)`.
    pub start: usize,
    pub end: usize,
    pub rank: u8,
    pub phrase: Phrase,
    /// The phrase's own words include a filler (so no preposition is left to clean up before it).
    pub consumed_filler: bool,
}

pub struct Found {
    pub best: Option<Cand>,
    /// Another phrase of the same rank that was passed over.
    pub second: Option<Cand>,
    pub no_unit: bool,
    pub bad_clock: bool,
}

pub fn find(toks: &[Tok], uk: bool, cyr: bool) -> Found {
    let cx = Cx { t: toks, uk, cyr, filler: std::cell::Cell::new(false) };
    let mut cands: Vec<Cand> = Vec::new();
    let mut no_unit = false;
    let mut bad_clock = false;
    let mut i = 0;
    while i < toks.len() {
        match cx.abs_at(i) {
            Abs::Yes(c) => {
                i = c.end;
                cands.push(c);
                continue;
            }
            Abs::Bad => bad_clock = true,
            Abs::No => {}
        }
        match cx.dur_at(i) {
            Dur::Yes(c) => {
                i = c.end;
                cands.push(c);
                continue;
            }
            Dur::NoUnit => no_unit = true,
            Dur::No => {}
        }
        i += 1;
    }
    // The best: highest rank, then the leftmost.
    let mut best: Option<usize> = None;
    for (k, c) in cands.iter().enumerate() {
        if best.is_none_or(|b| c.rank > cands[b].rank) {
            best = Some(k);
        }
    }
    let Some(b) = best else { return Found { best: None, second: None, no_unit, bad_clock } };
    let best_c = cands[b].clone();
    let second = cands.iter().enumerate().find(|(k, c)| *k != b && c.rank == best_c.rank && c.rank >= 2).map(|(_, c)| c.clone());
    Found { best: Some(best_c), second, no_unit: false, bad_clock: false }
}

enum Abs {
    No,
    Bad,
    Yes(Cand),
}

enum Dur {
    No,
    NoUnit,
    Yes(Cand),
}

struct Cx<'a> {
    t: &'a [Tok],
    uk: bool,
    cyr: bool,
    /// A duration word ("через", "in") came before: a lone `с` after a number is then really seconds.
    filler: std::cell::Cell<bool>,
}

/// One duration component: its length, its unit and where the next token is.
#[derive(Clone, Copy)]
struct Comp {
    secs: f64,
    unit: Unit,
    next: usize,
}

impl<'a> Cx<'a> {
    fn w(&self, i: usize) -> &str {
        self.t.get(i).map(|t| t.word()).unwrap_or("")
    }

    fn words_from(&self, i: usize) -> Vec<&str> {
        self.t.iter().skip(i).take(4).map(|t| t.word()).collect()
    }

    fn unit_at(&self, i: usize) -> Option<Unit> {
        let t = self.t.get(i)?;
        if !t.is_word() {
            return None;
        }
        // "в 10 с командой": a lone с between words is the preposition, not seconds.
        if t.norm == "с" && !t.glued && !self.filler.get() && self.t.get(i + 1).is_some_and(|n| n.is_word()) {
            return None;
        }
        words::unit(&t.norm, self.cyr)
    }

    /// A unit at `i`, allowing one glued hyphen before it (`15-min`).
    fn unit_after_number(&self, i: usize) -> Option<(Unit, usize)> {
        if let Some(u) = self.unit_at(i) {
            return Some((u, i + 1));
        }
        if self.t.get(i).is_some_and(|t| t.is_punct('-') && t.glued) {
            if let Some(u) = self.unit_at(i + 1) {
                if self.t[i + 1].glued {
                    return Some((u, i + 2));
                }
            }
        }
        None
    }

    // ───────────────────────── durations ─────────────────────────

    /// A number at `i`: digits, a decimal, words (`двадцать пять`), a vague amount (`a couple of`, `пару`) or an
    /// article. Returns the value, the next token and whether it was an article or vague amount.
    fn number_at(&self, i: usize, article_ok: bool) -> Option<(f64, usize, bool)> {
        let t = self.t.get(i)?;
        match t.kind {
            Kind::Int(n) if n != u32::MAX => return Some((n as f64, i + 1, false)),
            Kind::Dec(v) => return Some((v, i + 1, false)),
            Kind::Word => {}
            _ => return None,
        }
        let w = t.norm.as_str();
        if let Some(q) = words::vague_quantity(w) {
            // "a couple of minutes", "a few minutes"
            let mut j = i + 1;
            if self.w(j) == "of" {
                j += 1;
            }
            return Some((q as f64, j, true));
        }
        if words::is_article(w) && self.w(i + 1) == "couple" {
            let mut j = i + 2;
            if self.w(j) == "of" {
                j += 1;
            }
            return Some((2.0, j, true));
        }
        if words::is_article(w) && self.w(i + 1) == "few" {
            return Some((3.0, i + 2, true));
        }
        if words::is_article(w) {
            return if article_ok { Some((1.0, i + 1, true)) } else { None };
        }
        // Words, skipping the glued hyphen of "twenty-five".
        let mut ws: Vec<&str> = Vec::new();
        let mut ends: Vec<usize> = Vec::new();
        let mut k = i;
        while ws.len() < 4 {
            match self.t.get(k) {
                Some(t) if t.is_word() => {
                    ws.push(t.norm.as_str());
                    k += 1;
                    ends.push(k);
                }
                Some(t) if t.is_punct('-') && t.glued && !ws.is_empty() && self.t.get(k + 1).is_some_and(|n| n.is_word() && n.glued) => k += 1,
                _ => break,
            }
        }
        let (n, used) = words::number_words(&ws)?;
        Some((n as f64, ends[used - 1], false))
    }

    /// "and a half" / "с половиной" at `i` (after a unit). Returns the next token.
    fn and_a_half(&self, i: usize, after_unit: bool) -> Option<usize> {
        let mut j = i;
        let w = self.w(j);
        if words::and_connector(w) || matches!(w, "с" | "з") {
            j += 1;
        }
        if words::is_article(self.w(j)) {
            j += 1;
        }
        let w = self.w(j);
        if !(words::is_half(w) || words::is_with_half(w)) {
            return None;
        }
        let unit_follows = self.unit_at(j + 1).is_some();
        // Before the unit ("two and a half hours") a unit must follow; after it ("an hour and a half") none may.
        if unit_follows == after_unit || matches!(self.w(j + 1), "an" | "a" | "of") {
            return None;
        }
        Some(j + 1)
    }

    fn component(&self, i: usize, allow_bare: bool) -> Option<Comp> {
        let t = self.t.get(i)?;
        // 1:15 and 0:01:30
        if let Kind::Clock { h, m, s, .. } = t.kind {
            let secs = h as f64 * 3600.0 + m as f64 * 60.0 + s.unwrap_or(0) as f64;
            return Some(Comp { secs, unit: if s.is_some() { Unit::Sec } else { Unit::Min }, next: i + 1 });
        }
        if t.is_word() {
            let w = t.norm.as_str();
            // полчаса, півгодини
            if let Some((u, k)) = words::half_word(w) {
                return Some(Comp { secs: u.secs() * k, unit: u, next: i + 1 });
            }
            // пол часа / пів години
            if words::half_prefix(w) {
                if let Some(u) = self.unit_at(i + 1) {
                    return Some(Comp { secs: u.secs() * 0.5, unit: u, next: i + 2 });
                }
            }
            // полтора часа / півтори години
            if words::one_and_half(w) {
                if let Some(u) = self.unit_at(i + 1) {
                    return Some(Comp { secs: u.secs() * 1.5, unit: u, next: i + 2 });
                }
            }
            // четверть часа / чверть години
            if words::is_quarter_word(w) {
                if let Some(u) = self.unit_at(i + 1) {
                    return Some(Comp { secs: u.secs() * 0.25, unit: u, next: i + 2 });
                }
            }
            // half an hour, half hour, half-hour
            let mut j = i;
            if words::is_article(w) && words::is_half(self.w(i + 1)) {
                j = i + 1;
            }
            if words::is_half(self.w(j)) {
                let mut k = j + 1;
                if self.t.get(k).is_some_and(|t| t.is_punct('-') && t.glued) {
                    k += 1;
                }
                if words::is_article(self.w(k)) {
                    k += 1;
                }
                if let Some(u) = self.unit_at(k) {
                    return Some(Comp { secs: u.secs() * 0.5, unit: u, next: k + 1 });
                }
            }
            // a quarter of an hour, quarter hour
            let mut j = i;
            if words::is_article(w) || w == "one" {
                j = i + 1;
            }
            if self.w(j) == "quarter" {
                let mut k = j + 1;
                if self.w(k) == "of" {
                    k += 1;
                }
                if words::is_article(self.w(k)) {
                    k += 1;
                }
                if let Some(u) = self.unit_at(k) {
                    return Some(Comp { secs: u.secs() * 0.25, unit: u, next: k + 1 });
                }
            }
        }
        // number [with half] unit
        let article_ok = allow_bare || self.article_with_half(i);
        if let Some((n, mut j, vague)) = self.number_at(i, article_ok) {
            let mut value = n;
            // три четверти часа, three quarters of an hour
            if words::is_quarters(self.w(j)) && !vague {
                let mut k = j + 1;
                if self.w(k) == "of" {
                    k += 1;
                }
                if words::is_article(self.w(k)) {
                    k += 1;
                }
                if let Some(u) = self.unit_at(k) {
                    return Some(Comp { secs: u.secs() * 0.25 * n, unit: u, next: k + 1 });
                }
            }
            // два с половиной часа, two and a half hours
            if !vague {
                if let Some(k) = self.and_a_half(j, false) {
                    if self.unit_at(k).is_some() {
                        value += 0.5;
                        j = k;
                    }
                }
            }
            let (u, mut next) = self.unit_after_number(j)?;
            let mut secs = value * u.secs();
            // an hour and a half, час с половиной
            if matches!(u, Unit::Hour | Unit::Min | Unit::Day) {
                if let Some(k) = self.and_a_half(next, true) {
                    secs += 0.5 * u.secs();
                    next = k;
                }
            }
            return Some(Comp { secs, unit: u, next: self.skip_dot(next) });
        }
        // a bare unit: "через час", "in hour"
        if (allow_bare || self.and_a_half(i + 1, true).is_some()) && t.is_word() && t.norm.chars().count() > 1 {
            if let Some(u) = self.unit_at(i) {
                let mut secs = u.secs();
                let mut next = i + 1;
                if matches!(u, Unit::Hour | Unit::Min | Unit::Day) {
                    if let Some(k) = self.and_a_half(next, true) {
                        secs += 0.5 * u.secs();
                        next = k;
                    }
                }
                return Some(Comp { secs, unit: u, next: self.skip_dot(next) });
            }
        }
        None
    }

    /// An abbreviation dot glued to the unit ("1 ч.", "15 мин.").
    fn skip_dot(&self, i: usize) -> usize {
        if self.t.get(i).is_some_and(|t| t.is_punct('.') && t.glued) { i + 1 } else { i }
    }

    /// `an hour and a half` without a filler: the article is allowed because of the "and a half".
    fn article_with_half(&self, i: usize) -> bool {
        words::is_article(self.w(i)) && self.unit_at(i + 1).is_some() && self.and_a_half(i + 2, true).is_some()
    }

    /// A run of components with strictly smaller units: `1ч 15м`, `1 hour and 15 minutes`, `1ч30`.
    fn duration_at(&self, i: usize, allow_bare: bool) -> Option<(f64, usize)> {
        let first = self.component(i, allow_bare)?;
        let mut total = first.secs;
        let mut last = first.unit;
        let mut j = first.next;
        // A clock duration (1:15) is complete in itself.
        if matches!(self.t[i].kind, Kind::Clock { .. }) {
            return Some((total, j));
        }
        loop {
            // `1ч30`: a glued bare number after a unit is the next smaller unit.
            if let (Some(below), Some(t)) = (last.below(), self.t.get(j)) {
                if let Kind::Int(n) = t.kind {
                    if t.glued && n < 60 && self.unit_at(j + 1).is_none() && !(self.t.get(j + 1).is_some_and(|x| x.is_punct(':'))) && self.t[j - 1].is_word() {
                        total += n as f64 * below.secs();
                        last = below;
                        j += 1;
                        continue;
                    }
                }
            }
            // A connector, then a component with a smaller unit.
            let mut k = j;
            if let Some(t) = self.t.get(k) {
                let w = t.word();
                if t.is_punct(',') || t.is_punct('+') || t.is_punct('&') || words::and_connector(w) {
                    k += 1;
                }
            }
            match self.component(k, k > j) {
                Some(c) if c.unit < last && !matches!(self.t[k].kind, Kind::Clock { .. }) => {
                    total += c.secs;
                    last = c.unit;
                    j = c.next;
                }
                _ => break,
            }
        }
        Some((total, j))
    }

    fn dur_at(&self, i: usize) -> Dur {
        let w = self.w(i);
        let mut j = i;
        let mut strong = false;
        let mut weak = false;
        if words::strong_dur_filler(w, self.uk) {
            strong = true;
            j += 1;
        } else if words::weak_dur_filler(w) {
            weak = true;
            j += 1;
        } else if words::approx_word(w) {
            j += 1;
        }
        if (strong || weak || j > i) && words::approx_word(self.w(j)) {
            j += 1;
        }
        let filler = strong || weak;
        self.filler.set(filler);
        let start_tok = &self.t[i];
        // A bare clock like 1:15 is a duration only here; "18:30" is decided in abs_at.
        let first_is_clock = matches!(self.t.get(j).map(|t| &t.kind), Some(Kind::Clock { .. }));
        let result = self.duration_at(j, filler);
        self.filler.set(false);
        match result {
            Some((secs, mut end)) => {
                // "15 minutes from now" / "через час спустя" are still durations; "from now" makes it certain.
                let tail = words::from_now_suffix(&self.t.iter().skip(end).take(2).map(|t| t.word()).collect::<Vec<_>>());
                if tail > 0 {
                    end += tail;
                    strong = true;
                }
                let consumed = strong || weak || j > i;
                // A clock-looking bare duration (`1:15`) needs the plausibility test used for absolute times.
                if first_is_clock && !consumed {
                    if let Kind::Clock { h, zero, s, .. } = start_tok.kind {
                        if s.is_none() && (h > 5 || zero) {
                            return Dur::No;
                        }
                    }
                }
                let _ = start_tok;
                Dur::Yes(Cand { start: i, end, rank: if strong { 3 } else { 2 }, phrase: Phrase::Dur { secs }, consumed_filler: consumed })
            }
            None => {
                if strong && self.number_at(j, false).is_some() {
                    Dur::NoUnit
                } else {
                    Dur::No
                }
            }
        }
    }

    // ───────────────────────── clock times ─────────────────────────

    fn day_at(&self, i: usize) -> Option<(DayWord, usize)> {
        if self.w(i) == "day" && self.w(i + 1) == "after" && self.w(i + 2) == "tomorrow" {
            return Some((DayWord::AfterTomorrow, i + 3));
        }
        let d = words::day_word(self.w(i))?;
        Some((d, i + 1))
    }

    /// The hour (and minutes) written at `i`. `loose` means a filler or a day word came first, so a bare hour counts.
    /// Returns (h, m, next, certain) where `certain` says the text has more than a bare number (a colon, a day
    /// part, am/pm), so it is not a coincidence.
    fn clock_at(&self, i: usize, loose: bool) -> Result<Option<(u32, u32, usize, bool)>, ()> {
        let Some(t) = self.t.get(i) else { return Ok(None) };
        let mut certain = false;
        let (mut h, m, mut j) = match &t.kind {
            Kind::Clock { h, m, s: None, .. } => {
                if *m > 59 || *h > 24 || (*h == 24 && *m != 0) {
                    return Err(());
                }
                certain = true;
                (*h, *m, i + 1)
            }
            Kind::Clock { .. } => return Ok(None),
            Kind::Dec(_) if loose => {
                // 18.30 / 18,30
                let text = t.text.replace(',', ".");
                let (a, b) = text.split_once('.').unwrap_or(("", ""));
                if b.len() != 2 || a.len() > 2 {
                    return Ok(None);
                }
                let (h, m): (u32, u32) = (a.parse().map_err(|_| ())?, b.parse().map_err(|_| ())?);
                if h > 23 || m > 59 {
                    return Err(());
                }
                certain = true;
                (h, m, i + 1)
            }
            Kind::Int(n) if *n != u32::MAX => {
                // an hour: only with a filler, or with am/pm and the like after it
                (*n, 0, i + 1)
            }
            Kind::Word => {
                let w = t.norm.as_str();
                if loose {
                    if let Some(h) = words::named_time(w) {
                        return Ok(Some((h, 0, i + 1, true)));
                    }
                    let ws = self.words_from(i);
                    match words::number_words(&ws) {
                        Some((n, used)) if n <= 24 => (n, 0, i + used),
                        _ => return Ok(None),
                    }
                } else {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        };
        let is_int_hour = matches!(t.kind, Kind::Int(_) | Kind::Word);
        // o'clock / часов
        let mut marker = false;
        if is_int_hour {
            if let Some(next) = self.t.get(j) {
                if next.is_word() && words::is_hour_marker(&next.norm) {
                    if !loose {
                        // "5 часов" without "at" is a length
                        return Ok(None);
                    }
                    marker = true;
                    j += 1;
                } else if loose && next.is_word() && words::day_part(&next.norm).is_some() {
                    // "в 3 дня": the day part, not a count of days
                } else if self.unit_at(j).is_some() {
                    // "в 5 минут", "at 3 days": a length, not a clock time
                    return Ok(None);
                }
            }
        }
        // am / pm
        let mut meridiem = None;
        if let Some(next) = self.t.get(j) {
            if next.is_word() && (words::is_am(&next.norm) || words::is_pm(&next.norm)) {
                meridiem = Some(words::is_pm(&next.norm));
                j += 1;
            }
        }
        // утра / вечера / ночи / дня
        let mut part = None;
        if meridiem.is_none() {
            if let Some(next) = self.t.get(j) {
                if next.is_word() {
                    if let Some(p) = words::day_part(&next.norm) {
                        part = Some(p);
                        j += 1;
                    }
                }
            }
        }
        if meridiem.is_none() && part.is_none() && h > 24 {
            return Ok(None);
        }
        if let Some(pm) = meridiem {
            if !(1..=12).contains(&h) {
                return Err(());
            }
            h = match (pm, h) {
                (true, 12) => 12,
                (true, h) => h + 12,
                (false, 12) => 0,
                (false, h) => h,
            };
            certain = true;
        } else if let Some(p) = part {
            if h > 12 {
                return Err(());
            }
            h = words::apply_day_part(h, p);
            certain = true;
        }
        if marker {
            certain = true;
        }
        if h > 24 || (h == 24 && m != 0) {
            return Err(());
        }
        if !loose && !certain {
            return Ok(None);
        }
        Ok(Some((h, m, j, certain)))
    }

    fn abs_at(&self, i: usize) -> Abs {
        let w = self.w(i);
        let mut j = i;
        let mut day = None;
        let mut filler = false;
        // "on monday", "в понедельник", "во вторник"
        let mut k = j;
        if matches!(w, "on" | "в" | "во" | "у" | "на") {
            if let Some((d @ DayWord::Weekday(_), _)) = self.day_at(i + 1) {
                if !words::is_short_weekday(self.w(i + 1)) || self.at_then_clock(i + 2) {
                    day = Some(d);
                    k = i + 1;
                }
            }
        }
        if day.is_none() {
            if let Some((d, next)) = self.day_at(i) {
                let short = matches!(d, DayWord::Weekday(_)) && words::is_short_weekday(w);
                if !short || self.at_then_clock(next) {
                    day = Some(d);
                    k = i;
                }
            }
        }
        let mut pre_part = None;
        if w == "tonight" {
            day = Some(DayWord::Today);
            pre_part = Some(words::DayPart::Evening);
            j = i + 1;
        } else if let Some(d) = day {
            // skip the day word(s), and a day part after them ("завтра вечером в 8")
            j = match self.day_at(k) {
                Some((_, next)) => next,
                None => k + 1,
            };
            let _ = d;
            if let Some(p) = words::day_part(self.w(j)) {
                pre_part = Some(p);
                j += 1;
            }
        } else if words::at_filler(w) || self.t[i].is_punct('@') {
            filler = true;
            j = i + 1;
        }
        if j == i && !matches!(self.t[i].kind, Kind::Clock { .. } | Kind::Int(_)) {
            return Abs::No;
        }
        // after a day word an "at" filler is optional
        if day.is_some() && words::at_filler(self.w(j)) {
            filler = true;
            j += 1;
        }
        let loose = filler || day.is_some();
        // A bare H:MM may be a length (1:15): let the duration reader have it.
        if !loose {
            if let Kind::Clock { h, zero, s, .. } = self.t[i].kind {
                if s.is_some() || (h <= 5 && !zero) {
                    return Abs::No;
                }
            }
        }
        match self.clock_at(j, loose) {
            Err(()) => {
                if loose || matches!(self.t[i].kind, Kind::Clock { .. }) {
                    Abs::Bad
                } else {
                    Abs::No
                }
            }
            Ok(None) => {
                // "tomorrow" alone has no time: not a phrase
                Abs::No
            }
            Ok(Some((mut h, m, end, certain))) => {
                if let Some(p) = pre_part {
                    if h <= 12 {
                        h = words::apply_day_part(h, p);
                    }
                }
                // After the clock: a day word on the other side ("at 9 tomorrow").
                let mut end = end;
                if day.is_none() {
                    if let Some((d, next)) = self.day_at(end) {
                        if !matches!(d, DayWord::Weekday(_)) {
                            day = Some(d);
                            end = next;
                        }
                    }
                }
                let rank = if day.is_some() || (filler && certain) {
                    3
                } else if filler {
                    1
                } else {
                    2
                };
                // A bare hour that is not after a filler can only be reached with a meridiem (checked in clock_at).
                Abs::Yes(Cand { start: i, end, rank, phrase: Phrase::At { h, m, day }, consumed_filler: filler || day.is_some() })
            }
        }
    }

    /// After a short weekday: an "at" word and a clock time.
    fn at_then_clock(&self, i: usize) -> bool {
        words::at_filler(self.w(i)) && matches!(self.clock_at(i + 1, true), Ok(Some(_)))
    }
}
