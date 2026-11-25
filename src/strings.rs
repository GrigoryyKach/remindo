//! All interface text, in English, Russian and Ukrainian. One row per string: `key: en, ru, uk;`.
//! Style: sentence case, no emoji, no trailing periods on labels, no exclamation marks.

use crate::i18n::Table;

macro_rules! strings {
    (
        text { $( $name:ident : $en:expr, $ru:expr, $uk:expr; )* }
    ) => {
        #[allow(dead_code)]
        pub struct S {
            $( pub $name: &'static str, )*
        }
        impl S {
            /// Every string with its key, for the completeness tests.
            #[cfg(test)]
            pub fn rows(&self) -> Vec<(&'static str, &'static str)> {
                vec![ $( (stringify!($name), self.$name), )* ]
            }
        }
        const EN: S = S { $( $name: $en, )* };
        const RU: S = S { $( $name: $ru, )* };
        const UK: S = S { $( $name: $uk, )* };
        pub static TABLE: Table<S> = Table { en: &EN, ru: &RU, uk: &UK };
    };
}

strings! {
    text {
        // Quick input
        placeholder: "A time and a note: in 15 min turn off the stove", "Время и заметка: через 15 минут выключить плиту", "Час і нотатка: через 15 хвилин вимкнути плиту";
        hint_no_time: "Add a time, e.g. «15 min» or «at 18:30»", "Добавьте время, например «15 мин» или «в 18:30»", "Додайте час, наприклад «15 хв» або «о 18:30»";
        hint_no_unit: "Add a unit, e.g. «15 min»", "Добавьте единицу, например «15 мин»", "Додайте одиницю, наприклад «15 хв»";
        hint_zero: "The time must be longer than zero", "Время должно быть больше нуля", "Час має бути більшим за нуль";
        hint_bad_clock: "That time does not exist", "Такого времени не бывает", "Такого часу не існує";
        hint_clamped: "The longest timer is 7 days, set to 7 days", "Самый длинный таймер: 7 дней, поставлено 7 дней", "Найдовший таймер: 7 днів, встановлено 7 днів";
        hint_ignored: "Used the first time, ignored «{t}»", "Взято первое время, «{t}» пропущено", "Взято перший час, «{t}» пропущено";
        at_time: "at {t}", "в {t}", "о {t}";
        tomorrow_at: "tomorrow at {t}", "завтра в {t}", "завтра о {t}";
        day_at: "{d} at {t}", "{d} в {t}", "{d} о {t}";
        in_time: "in {d}", "через {d}", "через {d}";
        start: "Start", "Запустить", "Запустити";
        key_start: "start", "запуск", "запуск";
        key_close: "close", "закрыть", "закрити";
        key_recall: "last entry", "прошлый ввод", "попередній ввід";
        add_failed: "Could not add the timer", "Не удалось добавить таймер", "Не вдалося додати таймер";
        too_many: "Too many active timers", "Слишком много активных таймеров", "Забагато активних таймерів";
        daemon_down: "Remindo is not running in the background", "Remindo не запущен в фоне", "Remindo не запущено у фоні";
        started_toast: "Timer set", "Таймер запущен", "Таймер запущено";

        // Fire window
        fire_title: "Timer finished", "Таймер сработал", "Таймер спрацював";
        fire_set: "set at {t} for {d}", "поставлен в {t} на {d}", "поставлено о {t} на {d}";
        fire_set_clock: "set at {t} for {c}", "поставлен в {t} на {c}", "поставлено о {t} на {c}";
        fire_missed: "missed at {t}", "пропущен в {t}", "пропущено о {t}";
        close: "Close", "Закрыть", "Закрити";
        repeat: "Repeat", "Повторить", "Повторити";
        snooze5: "+5 min", "+5 мин", "+5 хв";
        close_all: "Close all", "Закрыть все", "Закрити всі";

        // Tray
        tray_new: "New timer", "Новый таймер", "Новий таймер";
        tray_active: "Active timers", "Активные таймеры", "Активні таймери";
        tray_settings: "Settings", "Настройки", "Налаштування";
        tray_exit: "Exit", "Выход", "Вихід";
        tray_tip_none: "no active timers", "нет активных таймеров", "немає активних таймерів";
        tray_tip_next: "next in {d}", "ближайший через {d}", "найближчий через {d}";

        // Settings window
        nav_timers: "Timers", "Таймеры", "Таймери";
        nav_history: "History", "История", "Історія";
        nav_settings: "Settings", "Настройки", "Налаштування";
        active_timers: "Active timers", "Активные таймеры", "Активні таймери";
        no_timers: "No active timers", "Нет активных таймеров", "Немає активних таймерів";
        no_timers_hint: "Press the shortcut and type a time and a note, for example «in 15 min tea»", "Нажмите сочетание клавиш и введите время и заметку, например «через 15 минут чай»", "Натисніть скорочення й введіть час і нотатку, наприклад «через 15 хвилин чай»";
        new_timer: "New timer", "Новый таймер", "Новий таймер";
        left: "left", "осталось", "залишилось";
        fires_at: "at {t}", "в {t}", "о {t}";
        cancel: "Cancel", "Отменить", "Скасувати";
        plus5: "+5 min", "+5 мин", "+5 хв";
        edit: "Edit", "Изменить", "Змінити";
        save: "Save", "Сохранить", "Зберегти";
        edit_hint: "Change the note, or type a new time as well", "Измените заметку или введите новое время", "Змініть нотатку або введіть новий час";
        history: "History", "История", "Історія";
        history_hint: "The last 50 timers", "Последние 50 таймеров", "Останні 50 таймерів";
        no_history: "Nothing here yet", "Пока пусто", "Поки порожньо";
        start_again: "Start again", "Запустить снова", "Запустити знову";
        clear_history: "Clear history", "Очистить историю", "Очистити історію";
        group_general: "General", "Основное", "Основне";
        hotkey: "Shortcut", "Сочетание клавиш", "Скорочення";
        hotkey_hint: "Press it anywhere to add a timer", "Нажмите его где угодно, чтобы добавить таймер", "Натисніть його будь-де, щоб додати таймер";
        hotkey_change: "Change", "Изменить", "Змінити";
        hotkey_recording: "Press the new shortcut", "Нажмите новое сочетание", "Натисніть нове скорочення";
        hotkey_cancel_hint: "Esc cancels", "Esc отменяет", "Esc скасовує";
        hotkey_reset: "Reset", "Сбросить", "Скинути";
        hotkey_saved: "Shortcut changed", "Сочетание изменено", "Скорочення змінено";
        hotkey_taken: "Could not use {hk}: another program has it. Kept {old}.", "Не удалось занять {hk}: оно занято другой программой. Оставлено {old}.", "Не вдалося зайняти {hk}: воно зайняте іншою програмою. Залишено {old}.";
        hotkey_down: "The shortcut could not be registered: another program has it. Choose another one.", "Не удалось занять сочетание: оно занято другой программой. Выберите другое.", "Не вдалося зайняти скорочення: воно зайняте іншою програмою. Оберіть інше.";
        hotkey_wayland: "On Wayland a program cannot grab keys. Bind a system shortcut to the command below.", "В Wayland программа не может перехватывать клавиши. Назначьте системное сочетание на команду ниже.", "У Wayland програма не може перехоплювати клавіші. Призначте системне скорочення на команду нижче.";
        sound: "Sound", "Звук", "Звук";
        sound_hint: "A soft chime when a timer finishes", "Тихий сигнал, когда таймер сработал", "Тихий сигнал, коли таймер спрацював";
        sound_play: "Play", "Проиграть", "Відтворити";
        autostart: "Start with Windows", "Запускать вместе с Windows", "Запускати разом із Windows";
        autostart_os: "Start at login", "Запускать при входе", "Запускати під час входу";
        autostart_hint: "Remindo waits in the tray so the shortcut always works", "Remindo ждёт в трее, чтобы сочетание клавиш всегда работало", "Remindo чекає в треї, щоб скорочення завжди працювало";
        autostart_failed: "Could not change the startup setting", "Не удалось изменить автозапуск", "Не вдалося змінити автозапуск";
        about: "About", "О программе", "Про програму";
        version: "Version", "Версия", "Версія";
        privacy_note: "Everything stays on this computer. The only network request is the optional check for updates on GitHub.", "Всё остаётся на этом компьютере. Единственный сетевой запрос: необязательная проверка обновлений на GitHub.", "Усе лишається на цьому комп'ютері. Єдиний мережевий запит: необов'язкова перевірка оновлень на GitHub.";
        daemon_off: "Remindo is not running in the background, so the shortcut and the timers are off", "Remindo не запущен в фоне, поэтому сочетание клавиш и таймеры не работают", "Remindo не запущено у фоні, тому скорочення й таймери не працюють";
        daemon_start: "Start", "Запустить", "Запустити";
        notice_timers_corrupt: "The timers file was damaged. It was moved to {name} and Remindo started with an empty list.", "Файл таймеров был повреждён. Он перемещён в {name}, Remindo начал с пустого списка.", "Файл таймерів було пошкоджено. Його переміщено в {name}, Remindo почав із порожнього списку.";
        notice_timers_newer: "The timers file was made by a newer version. It was moved to {name} untouched.", "Файл таймеров создан более новой версией. Он перемещён в {name} без изменений.", "Файл таймерів створено новішою версією. Його переміщено в {name} без змін.";
        notice_timers_migrated: "Your timers were upgraded to the new format. A backup is in {name}.", "Таймеры обновлены до нового формата. Резервная копия: {name}.", "Таймери оновлено до нового формату. Резервна копія: {name}.";
        notice_config_corrupt: "The settings file was damaged. It was moved to {name} and the defaults are used.", "Файл настроек был повреждён. Он перемещён в {name}, используются значения по умолчанию.", "Файл налаштувань було пошкоджено. Його переміщено в {name}, використано типові значення.";
        dismiss: "Dismiss", "Скрыть", "Сховати";
        days_short: "d", "д", "д";
        hours_short: "h", "ч", "год";
        minutes_short: "min", "мин", "хв";
        seconds_short: "s", "с", "с";
        today: "today", "сегодня", "сьогодні";
        tomorrow: "tomorrow", "завтра", "завтра";
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;

    fn placeholders(s: &str) -> Vec<String> {
        let mut v: Vec<String> = s.split('{').skip(1).filter_map(|p| p.split('}').next()).map(String::from).collect();
        v.sort();
        v
    }

    #[test]
    fn every_string_exists_in_all_three_languages_with_the_same_placeholders() {
        let en = TABLE.get(Lang::En).rows();
        let ru = TABLE.get(Lang::Ru).rows();
        let uk = TABLE.get(Lang::Uk).rows();
        assert!(en.len() > 60);
        for i in 0..en.len() {
            let (key, e) = en[i];
            let (r, u) = (ru[i].1, uk[i].1);
            assert!(!e.is_empty() && !r.is_empty() && !u.is_empty(), "{key} is empty in a language");
            assert_eq!(placeholders(e), placeholders(r), "{key}: placeholders differ between en and ru");
            assert_eq!(placeholders(e), placeholders(u), "{key}: placeholders differ between en and uk");
            // The other languages must actually be translated (an identical string is only fine for short symbols).
            if e.chars().count() > 12 && !matches!(key, "hint_no_unit") {
                assert!(e != r && e != u, "{key} looks untranslated");
            }
            for (lang, text) in [("en", e), ("ru", r), ("uk", u)] {
                assert!(!text.contains('!') || key == "dismiss", "{key}/{lang}: no exclamation marks in the interface");
                assert!(!text.chars().any(|c| c as u32 >= 0x1F000), "{key}/{lang}: no emoji");
                assert_eq!(text, text.trim(), "{key}/{lang}: stray spaces");
            }
        }
    }

    #[test]
    fn russian_and_ukrainian_are_not_mixed_up() {
        // Ukrainian-only letters never appear in the Russian table and the other way round.
        for (key, text) in TABLE.get(Lang::Ru).rows() {
            assert!(!text.chars().any(|c| matches!(c, 'і' | 'ї' | 'є' | 'ґ')), "{key}: Ukrainian letter in the Russian text: {text}");
        }
        for (key, text) in TABLE.get(Lang::Uk).rows() {
            assert!(!text.chars().any(|c| matches!(c, 'ы' | 'э' | 'ъ')), "{key}: Russian letter in the Ukrainian text: {text}");
        }
    }
}
