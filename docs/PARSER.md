# The parser

`parse::parse(input, now, ui_lang) -> Result<Parsed, Error>` turns free text into a fire time and a note. No network, no model; the same input and the same `now` always give the same result.

## Pipeline

1. **`lex`** splits the text into tokens: integers, decimals (`2.5`, `2,5`), clocks (`1:15`, `18:30`, `0:01:30`), words (letters and inner apostrophes) and single punctuation characters. Every token remembers its byte range in the original text and whether it was glued to the previous one (`1ч15м` is `1` `ч` `15` `м`, all glued). Words are normalised: lowercase, `ё` to `е`, apostrophes removed (`п'ять` to `пять`).
2. **`phrase`** finds every *time phrase*:
   - a **length** (`через 1 час 15 минут`, `1ч15м`, `полчаса`, `in an hour and a half`): a run of components with strictly decreasing units, joined by nothing, `and` / `и` / `і`, `,` or `+`. A component is a number (digits, a decimal, number words, an article, `пару`) and a unit, a half or a quarter phrase, a bare unit after a filler (`через час`), or a clock-shaped `H:MM`;
   - a **clock time** (`at 18:30`, `в 6 вечера`, `завтра в 9`, `6pm`, `on friday at 10`): an optional day word (and a day part), an optional "at" word, then `H:MM`, `H.MM`, an hour with `am`/`pm`, an hour with a day part (`утра` `дня` `вечера` `ночи` and the Ukrainian forms), an hour with `часов`, a number word, or `noon` / `midnight` / `полдень` / `полночь`.
   Each phrase gets a rank: 3 when a word introduces it (`через`, `in`, `at`, `в`) or it carries a day; 2 when it is bare (`15м`, `18:30`, `6pm`); 1 for a bare hour after "at" (`в 5`).
3. **`mod`** takes the best phrase (highest rank, then the leftmost), cuts it out of the original text, and cleans what is left into the note: separators at the cut are dropped, `remind me (to)` / `напомни (мне)` / `нагадай (мені)` are stripped at the start or the end, brackets and quotes that held only the phrase go with it, a preposition left right before the phrase goes with it, and an empty result stays empty (the caller shows `Timer` / «Таймер»). A second phrase of the same rank stays in the note and is reported as `Note::IgnoredSecond`.
4. The phrase becomes a fire time: a length is added to `now`; a clock time is the next occurrence (`Note::Tomorrow` when it moved because the time had passed; a weekday is the next one). More than 7 days is cut to 7 (`Note::ClampedMax`); zero is `Error::Zero`.

## Errors

| Error | When | Hint in the quick input |
|---|---|---|
| `Empty` | nothing typed | the generic hint |
| `NoTime` | no time found | "Add a time, e.g. «15 min» or «at 18:30»" |
| `NoUnit` | a length word with a number and no unit (`in 5`) | "Add a unit, e.g. «15 min»" |
| `Zero` | `0 min` | "The time must be longer than zero" |
| `BadClock` | `at 25:00`, `at 13 pm` | "That time does not exist" |

## Language

The language of the text decides the default note. It is guessed from evidence in the words (Ukrainian-only letters and words, Russian-only ones, Latin text); when the text does not say (`15m`), the interface language decides. `за` is a length filler only when the text is Ukrainian.

## Tests

`src/parse/tests.rs` is a table of more than 450 `(input, expected)` rows with the clock fixed to Wednesday 2026-11-25 14:00, split into English lengths, English clock times, compact forms, Russian lengths, Russian clock times, Ukrainian lengths, Ukrainian clock times, mixed languages and look-alikes, inputs that must not parse, the 7 day limit, "past means tomorrow", two phrases, default notes, note case and wording, and real-world input. A last test feeds 30 000 random mixtures of the parser's own vocabulary and noise through it and checks it never panics and that every result is between 1 second and 7 days.
