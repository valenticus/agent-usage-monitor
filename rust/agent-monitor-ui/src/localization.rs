use crate::settings::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    Uk,
    En,
    Pl,
}

pub fn system_language() -> Option<String> {
    #[cfg(windows)]
    {
        let mut buffer = [0u16; 85];
        let length = unsafe {
            windows_sys::Win32::Globalization::GetUserDefaultLocaleName(
                buffer.as_mut_ptr(),
                buffer.len() as i32,
            )
        };
        if length > 1 {
            return Some(String::from_utf16_lossy(&buffer[..(length - 1) as usize]));
        }
    }
    std::env::var("LANG").ok()
}

impl Locale {
    pub fn resolve(language: Language, system_language: Option<&str>) -> Self {
        match language {
            Language::Ukrainian => Self::Uk,
            Language::Polish => Self::Pl,
            Language::English => Self::En,
            Language::System => match system_language
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str()
            {
                value if value.starts_with("uk") => Self::Uk,
                value if value.starts_with("pl") => Self::Pl,
                _ => Self::En,
            },
        }
    }

    pub fn popup(self) -> PopupStrings {
        match self {
            Self::Uk => PopupStrings {
                five_hour: "5 год",
                weekly: "Тиждень",
                waiting: "Очікування",
                loading: "Завантаження",
                fresh: "Свіжі дані",
                stale: "Дані застаріли",
                unavailable: "Недоступно",
                reset_unknown: "Скидання невідоме",
                demo: "Демо · приклад",
            },
            Self::En => PopupStrings {
                five_hour: "5 hours",
                weekly: "Weekly",
                waiting: "Waiting",
                loading: "Loading",
                fresh: "Fresh data",
                stale: "Stale data",
                unavailable: "Unavailable",
                reset_unknown: "Reset unknown",
                demo: "Demo · sample",
            },
            Self::Pl => PopupStrings {
                five_hour: "5 godz.",
                weekly: "Tydzień",
                waiting: "Oczekiwanie",
                loading: "Ładowanie",
                fresh: "Aktualne dane",
                stale: "Nieaktualne",
                unavailable: "Niedostępne",
                reset_unknown: "Nieznany reset",
                demo: "Demo · próbka",
            },
        }
    }

    pub fn settings(self) -> SettingsStrings {
        match self {
            Self::Uk => SettingsStrings {
                title: "Налаштування",
                general: "Загальне",
                providers: "Провайдери",
                notifications: "Сповіщення",
                about: "Про програму",
                appearance: "Вигляд",
                theme: "Тема",
                language: "Мова",
                codex: "Codex",
                claude: "Claude",
                alerts: "Сповіщення",
                warning: "Поріг попередження",
                critical: "Критичний поріг",
                exhausted: "Сповіщати про вичерпання",
                weekly: "Тижневі ліміти",
                duration: "Тривалість вікна",
                alerts_description: "Показувати сповіщення Windows, коли ліміт майже вичерпано",
                claude_note: "Дані Claude потребують явно налаштованої команди Claude Code status line. AUM не змінює параметри Claude.",
                about_tagline: "Ліміти під рукою. Дані залишаються локальними.",
                version: "Версія",
                author: "Автор:",
                repository: "Репозиторій GitHub",
                license: "Ліцензія в метаданих пакета: MIT; файл LICENSE не включено",
                third_party: "Сторонні компоненти: Slint Community Edition",
                on: "Увімкнено",
                off: "Вимкнено",
                integration: "Стан інтеграції",
                system: "Системна",
                dark: "Темна",
                light: "Світла",
                system_language: "Системна",
                popup_seconds: "с",
                about_third: "Сторонній компонент: Slint Community Edition",
            },
            Self::En => SettingsStrings {
                title: "Settings",
                general: "GENERAL",
                providers: "PROVIDERS",
                notifications: "NOTIFICATIONS",
                about: "ABOUT",
                appearance: "Appearance",
                theme: "Theme",
                language: "Language",
                codex: "Codex",
                claude: "Claude",
                alerts: "Alerts",
                warning: "Warning threshold",
                critical: "Critical threshold",
                exhausted: "Exhausted alerts",
                weekly: "Weekly limits",
                duration: "Popup duration",
                alerts_description: "Show a Windows notification when a limit is running low",
                claude_note: "Claude quota data requires an explicitly configured Claude Code status-line command. AUM does not change Claude settings.",
                about_tagline: "Usage at a glance. Local by design.",
                version: "Version",
                author: "Author:",
                repository: "GitHub repository",
                license: "Package metadata: MIT; no LICENSE file is included",
                third_party: "Third-party: Slint Community Edition",
                on: "On",
                off: "Off",
                integration: "Integration status",
                system: "System",
                dark: "Dark",
                light: "Light",
                system_language: "System",
                popup_seconds: "sec",
                about_third: "Third-party: Slint Community Edition",
            },
            Self::Pl => SettingsStrings {
                title: "Ustawienia",
                general: "OGÓLNE",
                providers: "DOSTAWCY",
                notifications: "POWIADOMIENIA",
                about: "O PROGRAMIE",
                appearance: "Wygląd",
                theme: "Motyw",
                language: "Język",
                codex: "Codex",
                claude: "Claude",
                alerts: "Alerty",
                warning: "Próg ostrzeżenia",
                critical: "Próg krytyczny",
                exhausted: "Alerty o wyczerpaniu",
                weekly: "Limity tygodniowe",
                duration: "Czas okna",
                alerts_description: "Pokazuj powiadomienia Windows przy niskim limicie",
                claude_note: "Dane Claude wymagają jawnie skonfigurowanej komendy Claude Code status line. AUM nie zmienia ustawień Claude.",
                about_tagline: "Limity pod ręką. Dane pozostają lokalne.",
                version: "Wersja",
                author: "Autor:",
                repository: "Repozytorium GitHub",
                license: "Metadane pakietu: MIT; plik LICENSE nie jest dołączony",
                third_party: "Komponenty zewnętrzne: Slint Community Edition",
                on: "Wł.",
                off: "Wył.",
                integration: "Status integracji",
                system: "Systemowy",
                dark: "Ciemny",
                light: "Jasny",
                system_language: "Systemowy",
                popup_seconds: "s",
                about_third: "Komponent zewnętrzny: Slint Community Edition",
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupStrings {
    pub five_hour: &'static str,
    pub weekly: &'static str,
    pub waiting: &'static str,
    pub loading: &'static str,
    pub fresh: &'static str,
    pub stale: &'static str,
    pub unavailable: &'static str,
    pub reset_unknown: &'static str,
    pub demo: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsStrings {
    pub title: &'static str,
    pub general: &'static str,
    pub providers: &'static str,
    pub notifications: &'static str,
    pub about: &'static str,
    pub appearance: &'static str,
    pub theme: &'static str,
    pub language: &'static str,
    pub codex: &'static str,
    pub claude: &'static str,
    pub alerts: &'static str,
    pub warning: &'static str,
    pub critical: &'static str,
    pub exhausted: &'static str,
    pub weekly: &'static str,
    pub duration: &'static str,
    pub alerts_description: &'static str,
    pub claude_note: &'static str,
    pub about_tagline: &'static str,
    pub version: &'static str,
    pub author: &'static str,
    pub repository: &'static str,
    pub license: &'static str,
    pub third_party: &'static str,
    pub on: &'static str,
    pub off: &'static str,
    pub integration: &'static str,
    pub system: &'static str,
    pub dark: &'static str,
    pub light: &'static str,
    pub system_language: &'static str,
    pub popup_seconds: &'static str,
    pub about_third: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_language_resolves_supported_locales_and_falls_back_to_english() {
        assert_eq!(Locale::resolve(Language::System, Some("uk-UA")), Locale::Uk);
        assert_eq!(Locale::resolve(Language::System, Some("pl-PL")), Locale::Pl);
        assert_eq!(Locale::resolve(Language::System, Some("de-DE")), Locale::En);
        assert_eq!(Locale::resolve(Language::System, None), Locale::En);
    }

    #[test]
    fn every_supported_locale_has_complete_settings_copy() {
        for locale in [Locale::Uk, Locale::En, Locale::Pl] {
            let strings = locale.settings();
            for value in [
                strings.general,
                strings.providers,
                strings.notifications,
                strings.about,
                strings.appearance,
                strings.theme,
                strings.language,
                strings.alerts,
                strings.warning,
                strings.critical,
                strings.exhausted,
                strings.weekly,
                strings.duration,
                strings.alerts_description,
                strings.claude_note,
                strings.about_tagline,
                strings.version,
                strings.author,
                strings.repository,
                strings.license,
                strings.third_party,
                strings.on,
                strings.off,
                strings.integration,
                strings.about_third,
            ] {
                assert!(!value.is_empty());
            }
        }
    }
}
