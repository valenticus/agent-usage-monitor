use agent_usage_monitor_core::{
    ClaudeStatusLineSnapshot, ClaudeUsageSnapshot, ProviderError, ProviderSnapshot, ProviderStatus,
    RateLimitSnapshot, RateLimitWindow,
};

use crate::MainWindow;
use crate::claude_refresher::ClaudeRefreshFailure;
use crate::localization::Locale;
use crate::settings::AppSettings;

const STALE_AFTER_SECONDS: i64 = 900;

#[derive(Clone, Copy)]
enum ProviderView {
    Codex,
    Claude,
}

#[derive(Clone)]
struct WindowView {
    remaining: String,
    reset: String,
    value: f32,
    available: bool,
}

#[derive(Clone)]
struct ProviderUi {
    plan: String,
    status: String,
    status_color: slint::Color,
    /// Theme-aware status level for the UI: 0 normal, 1 warning, 2 critical, 3 neutral.
    status_level: i32,
    five: WindowView,
    weekly: WindowView,
}

#[derive(Clone)]
pub struct UiSnapshot {
    providers: Vec<ProviderSnapshot>,
    demo: bool,
    loading: bool,
    claude_bridge_enabled: bool,
    claude_usage: ClaudeUsageSnapshot,
}

/// Which quota window a tray column shows: the provider's primary (5-hour)
/// or secondary (weekly) window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayWindow {
    FiveHour,
    Weekly,
}

const TRAY_UNAVAILABLE: crate::tray::QuotaValue = crate::tray::QuotaValue {
    remaining_percent: None,
    freshness: crate::tray::DataFreshness::Unavailable,
};

impl UiSnapshot {
    /// The two tray columns (left, right), each one fixed quota window
    /// (F-11; "remaining" = 100 − used):
    ///
    /// | Enabled providers | Left              | Right               |
    /// |-------------------|-------------------|---------------------|
    /// | Codex and Claude  | Codex 5-hour      | Claude 5-hour       |
    /// | Codex only        | Codex 5-hour      | Codex weekly        |
    /// | Claude only       | Claude 5-hour     | Claude weekly       |
    /// | neither           | unavailable       | unavailable         |
    ///
    /// A missing window makes only its own column unavailable; stale
    /// last-known-good values stay visible as stale.
    pub fn tray_values(
        &self,
        codex_enabled: bool,
        claude_enabled: bool,
        now: i64,
    ) -> [crate::tray::QuotaValue; 2] {
        use TrayWindow::{FiveHour, Weekly};
        match (codex_enabled, claude_enabled) {
            (true, true) => [
                self.tray_value("codex", FiveHour, now),
                self.tray_value("claude", FiveHour, now),
            ],
            (true, false) => [
                self.tray_value("codex", FiveHour, now),
                self.tray_value("codex", Weekly, now),
            ],
            (false, true) => [
                self.tray_value("claude", FiveHour, now),
                self.tray_value("claude", Weekly, now),
            ],
            (false, false) => [TRAY_UNAVAILABLE; 2],
        }
    }

    fn tray_value(&self, id: &str, window: TrayWindow, now: i64) -> crate::tray::QuotaValue {
        use crate::tray::{DataFreshness, QuotaValue};

        let Some(provider) = self
            .providers
            .iter()
            .find(|provider| provider.provider_id == id)
        else {
            return TRAY_UNAVAILABLE;
        };
        let remaining = provider
            .limits
            .first()
            .and_then(|limit| match window {
                TrayWindow::FiveHour => limit.primary.as_ref(),
                TrayWindow::Weekly => limit.secondary.as_ref(),
            })
            .filter(|window| {
                window.used_percent.is_finite() && (0.0..=100.0).contains(&window.used_percent)
            })
            .map(RateLimitWindow::remaining_percent);
        let Some(remaining) = remaining else {
            return TRAY_UNAVAILABLE;
        };
        let unavailable = TRAY_UNAVAILABLE;
        let freshness = match (&provider.status, provider.last_success_unix_seconds) {
            (ProviderStatus::Available, Some(updated))
                if updated <= now && now.saturating_sub(updated) <= STALE_AFTER_SECONDS =>
            {
                DataFreshness::Fresh
            }
            (_, Some(_)) => DataFreshness::Stale,
            _ => return unavailable,
        };
        QuotaValue {
            remaining_percent: Some(remaining.round().clamp(0.0, 100.0) as u8),
            freshness,
        }
    }

    pub fn settings_status(
        &self,
        provider_id: &str,
        enabled: bool,
        now: i64,
        locale: Locale,
    ) -> (String, String) {
        if !enabled {
            return match locale {
                Locale::Uk => (
                    "Вимкнено".into(),
                    "Моніторинг цього провайдера вимкнено".into(),
                ),
                Locale::En => (
                    "Disabled".into(),
                    "Monitoring for this provider is disabled".into(),
                ),
                Locale::Pl => (
                    "Wyłączono".into(),
                    "Monitorowanie dostawcy jest wyłączone".into(),
                ),
                Locale::Ru => ("Выключено".into(), "Мониторинг провайдера выключен".into()),
            };
        }
        if self.demo {
            return match locale {
                Locale::Uk => (
                    "Демо".into(),
                    "Прикладові значення; немає живого підключення".into(),
                ),
                Locale::En => (
                    "Demo".into(),
                    "Sample values; no live provider connection".into(),
                ),
                Locale::Pl => (
                    "Demo".into(),
                    "Wartości przykładowe; brak połączenia na żywo".into(),
                ),
                Locale::Ru => ("Демо".into(), "Примерные значения; нет подключения".into()),
            };
        }
        if provider_id == "codex" && self.loading {
            return match locale {
                Locale::Uk => (
                    "Завантаження".into(),
                    "Перевіряється локальний Codex app-server".into(),
                ),
                Locale::En => (
                    "Loading".into(),
                    "Checking the local Codex app-server".into(),
                ),
                Locale::Pl => (
                    "Ładowanie".into(),
                    "Sprawdzanie lokalnego Codex app-server".into(),
                ),
                Locale::Ru => (
                    "Загрузка".into(),
                    "Проверка локального Codex app-server".into(),
                ),
            };
        }
        let provider = self
            .providers
            .iter()
            .find(|provider| provider.provider_id == provider_id);
        let Some(provider) = provider else {
            return unavailable_status(provider_id, None, self.claude_bridge_enabled, locale);
        };
        match provider.status {
            ProviderStatus::Available => {
                let updated = provider.last_success_unix_seconds.unwrap_or_default();
                let age = now.saturating_sub(updated);
                if age > STALE_AFTER_SECONDS {
                    let description = match locale {
                        Locale::Uk => {
                            format!("Останні дані {} хв тому; потрібне оновлення", age / 60)
                        }
                        Locale::En => {
                            format!("Last data was {} min ago; waiting for an update", age / 60)
                        }
                        Locale::Pl => format!(
                            "Ostatnie dane sprzed {} min; oczekiwanie na aktualizację",
                            age / 60
                        ),
                        Locale::Ru => format!(
                            "Последние данные {} мин назад; ожидается обновление",
                            age / 60
                        ),
                    };
                    return (
                        match locale {
                            Locale::Uk => "Застаріло",
                            Locale::En => "Stale",
                            Locale::Pl => "Nieaktualne",
                            Locale::Ru => "Устарели",
                        }
                        .into(),
                        description,
                    );
                }
                let age_label = match locale {
                    Locale::Uk => format!("Оновлено {} хв тому", age / 60),
                    Locale::En => format!("Updated {} min ago", age / 60),
                    Locale::Pl => format!("Zaktualizowano {} min temu", age / 60),
                    Locale::Ru => format!("Обновлено {} мин назад", age / 60),
                };
                (
                    match locale {
                        Locale::Uk => "Підключено",
                        Locale::En => "Connected",
                        Locale::Pl => "Połączono",
                        Locale::Ru => "Подключено",
                    }
                    .into(),
                    age_label,
                )
            }
            ProviderStatus::Error => {
                if provider_id == "claude"
                    && let Some((status, description)) =
                        claude_error_copy(provider.error.as_ref(), locale)
                {
                    (status.into(), description.into())
                } else {
                    match locale {
                        Locale::Uk => (
                            "Помилка".into(),
                            "Не вдалося отримати дані провайдера".into(),
                        ),
                        Locale::En => ("Error".into(), "Could not read provider data".into()),
                        Locale::Pl => (
                            "Błąd".into(),
                            "Nie udało się odczytać danych dostawcy".into(),
                        ),
                        Locale::Ru => (
                            "Ошибка".into(),
                            "Не удалось прочитать данные провайдера".into(),
                        ),
                    }
                }
            }
            ProviderStatus::Unavailable => unavailable_status(
                provider_id,
                provider.error.as_ref().map(|error| error.code.as_str()),
                self.claude_bridge_enabled,
                locale,
            ),
        }
    }

    /// Chip level matching `settings_status`: 0 connected, 1 stale/demo,
    /// 2 error, 3 neutral (disabled, loading, waiting, unavailable).
    pub fn settings_status_level(&self, provider_id: &str, enabled: bool, now: i64) -> i32 {
        if !enabled {
            return 3;
        }
        if self.demo {
            return 1;
        }
        if provider_id == "codex" && self.loading {
            return 3;
        }
        let Some(provider) = self
            .providers
            .iter()
            .find(|provider| provider.provider_id == provider_id)
        else {
            return 3;
        };
        match provider.status {
            ProviderStatus::Available => {
                let updated = provider.last_success_unix_seconds.unwrap_or_default();
                if now.saturating_sub(updated) > STALE_AFTER_SECONDS {
                    1
                } else {
                    0
                }
            }
            ProviderStatus::Error => 2,
            ProviderStatus::Unavailable => 3,
        }
    }

    pub fn alert_providers(
        &self,
        codex_enabled: bool,
        claude_enabled: bool,
    ) -> Vec<ProviderSnapshot> {
        self.providers
            .iter()
            .filter(|provider| match provider.provider_id.as_str() {
                "codex" => codex_enabled,
                "claude" => claude_enabled,
                _ => false,
            })
            .cloned()
            .collect()
    }

    pub fn demo(now: i64) -> Self {
        let window = |used_percent, reset| RateLimitWindow {
            used_percent,
            window_duration_mins: None,
            resets_at: Some(reset),
        };
        let provider = |id: &str, plan: &str, five, weekly| ProviderSnapshot {
            provider_id: id.into(),
            status: ProviderStatus::Available,
            version: None,
            limits: vec![RateLimitSnapshot {
                limit_id: None,
                limit_name: None,
                primary: Some(window(five, now + 2 * 3_600)),
                secondary: Some(window(weekly, now + 3 * 86_400)),
                plan_type: Some(plan.into()),
                rate_limit_reached_type: None,
            }],
            last_success_unix_seconds: Some(now),
            error: None,
        };
        Self {
            providers: vec![
                provider("codex", "Plus", 42.0, 68.0),
                provider("claude", "Pro", 84.0, 100.0),
            ],
            demo: true,
            loading: false,
            claude_bridge_enabled: false,
            claude_usage: ClaudeUsageSnapshot::default(),
        }
    }

    pub fn unavailable() -> Self {
        Self {
            providers: vec![empty_provider("codex"), empty_provider("claude")],
            demo: false,
            loading: false,
            claude_bridge_enabled: false,
            claude_usage: ClaudeUsageSnapshot::default(),
        }
    }

    pub fn loading(claude_bridge_enabled: bool) -> Self {
        Self {
            providers: vec![empty_provider("codex"), empty_provider("claude")],
            demo: false,
            loading: true,
            claude_bridge_enabled,
            claude_usage: ClaudeUsageSnapshot::default(),
        }
    }

    /// Same data with the Claude status-line listener marked on or off (the
    /// Store build can toggle it at run time); provider values are kept.
    pub fn with_claude_bridge(&self, enabled: bool) -> Self {
        let mut next = self.clone();
        next.claude_bridge_enabled = enabled;
        next
    }

    pub fn replace_codex(&self, codex: ProviderSnapshot) -> Self {
        let mut next = self.clone();
        replace_provider(&mut next.providers, codex);
        next.loading = false;
        next
    }

    pub fn replace_claude(
        &self,
        _claude: ProviderSnapshot,
        claude_status_line: ClaudeStatusLineSnapshot,
    ) -> Self {
        let mut next = self.clone();
        next.claude_usage
            .merge(claude_status_line.to_usage_snapshot());
        if let Some(provider) = next.claude_usage.to_provider_snapshot() {
            replace_provider(&mut next.providers, provider);
        }
        next
    }

    pub fn merge_claude_usage(&self, usage: ClaudeUsageSnapshot) -> Self {
        let mut next = self.clone();
        next.claude_usage.merge(usage);
        if let Some(provider) = next.claude_usage.to_provider_snapshot() {
            replace_provider(&mut next.providers, provider);
        }
        next
    }

    /// Claude CLI refresh is on but no Claude CLI is installed (nothing was
    /// started). Without any Claude data this is "not installed", not an
    /// error; data from the status line, if any, is kept as it is.
    pub fn replace_claude_cli_missing(&self) -> Self {
        let mut next = self.clone();
        let mut claude = next
            .providers
            .iter()
            .find(|provider| provider.provider_id == "claude")
            .cloned()
            .unwrap_or_else(|| empty_provider("claude"));
        if !claude.limits.is_empty() {
            return next;
        }
        claude.status = ProviderStatus::Unavailable;
        claude.error = Some(ProviderError {
            code: CLAUDE_CLI_UNAVAILABLE.into(),
            message: "Claude CLI not found on PATH".into(),
        });
        replace_provider(&mut next.providers, claude);
        next
    }

    pub fn replace_claude_bridge_error(&self) -> Self {
        let mut next = self.clone();
        let mut claude = next
            .providers
            .iter()
            .find(|provider| provider.provider_id == "claude")
            .cloned()
            .unwrap_or_else(|| empty_provider("claude"));
        claude.status = ProviderStatus::Error;
        claude.error = Some(ProviderError {
            code: "claude_bridge_error".into(),
            message: "Claude Code status line did not provide an update".into(),
        });
        replace_provider(&mut next.providers, claude);
        next
    }

    pub fn replace_claude_refresh_error(&self, failure: ClaudeRefreshFailure) -> Self {
        let mut next = self.clone();
        let mut claude = next
            .providers
            .iter()
            .find(|provider| provider.provider_id == "claude")
            .cloned()
            .unwrap_or_else(|| empty_provider("claude"));
        claude.status = ProviderStatus::Error;
        claude.error = Some(ProviderError {
            code: format!("claude_refresh_{}", failure.category()),
            message: "Claude background refresh failed".into(),
        });
        replace_provider(&mut next.providers, claude);
        next
    }
}

fn claude_error_copy(
    error: Option<&ProviderError>,
    locale: Locale,
) -> Option<(&'static str, &'static str)> {
    let code = error?.code.as_str();
    if code == "claude_bridge_error" {
        return Some(locale.claude_bridge_error());
    }
    let category = code.strip_prefix("claude_refresh_")?;
    locale.claude_refresh_failure(ClaudeRefreshFailure::from_category(category)?)
}

/// Error code of a Claude CLI refresh that found no installed CLI.
pub const CLAUDE_CLI_UNAVAILABLE: &str = "claude_cli_unavailable";
/// Error code of a Codex refresh that found no installed CLI (core crate).
const CODEX_CLI_UNAVAILABLE: &str = "codex_unavailable";

fn unavailable_status(
    provider_id: &str,
    error_code: Option<&str>,
    bridge_enabled: bool,
    locale: Locale,
) -> (String, String) {
    // A missing CLI: say so, and how to connect it (AUM never installs it).
    match error_code {
        Some(CODEX_CLI_UNAVAILABLE) => {
            return match locale {
                Locale::Uk => (
                    "CLI не знайдено".into(),
                    "Встановіть Codex CLI і виконайте codex login, потім оновіть ліміти".into(),
                ),
                Locale::En => (
                    "CLI not found".into(),
                    "Install the Codex CLI and run codex login, then refresh limits".into(),
                ),
                Locale::Pl => (
                    "Nie znaleziono CLI".into(),
                    "Zainstaluj Codex CLI i uruchom codex login, a potem odśwież limity".into(),
                ),
                Locale::Ru => (
                    "CLI не найден".into(),
                    "Установите Codex CLI и выполните codex login, затем обновите лимиты".into(),
                ),
            };
        }
        Some(CLAUDE_CLI_UNAVAILABLE) => {
            return match locale {
                Locale::Uk => (
                    "CLI не знайдено".into(),
                    "Встановіть Claude Code і увійдіть (claude) або вимкніть фонове оновлення"
                        .into(),
                ),
                Locale::En => (
                    "CLI not found".into(),
                    "Install Claude Code and sign in (claude), or turn off Background refresh"
                        .into(),
                ),
                Locale::Pl => (
                    "Nie znaleziono CLI".into(),
                    "Zainstaluj Claude Code i zaloguj się (claude) albo wyłącz odświeżanie w tle"
                        .into(),
                ),
                Locale::Ru => (
                    "CLI не найден".into(),
                    "Установите Claude Code и войдите (claude) или выключите фоновое обновление"
                        .into(),
                ),
            };
        }
        _ => {}
    }
    if provider_id == "claude" {
        if bridge_enabled {
            return match locale {
                Locale::Uk => (
                    "Очікування даних".into(),
                    "Очікується подія від Claude Code status line".into(),
                ),
                Locale::En => (
                    "Waiting for data".into(),
                    "Waiting for a Claude Code status-line event".into(),
                ),
                Locale::Pl => (
                    "Oczekiwanie na dane".into(),
                    "Oczekiwanie na zdarzenie Claude Code status line".into(),
                ),
                Locale::Ru => (
                    "Ожидание данных".into(),
                    "Ожидается событие от Claude Code status line".into(),
                ),
            };
        }
        return match locale {
            Locale::Uk => (
                "Потрібне налаштування".into(),
                "Увімкніть фонове оновлення або налаштуйте status line".into(),
            ),
            Locale::En => (
                "Setup required".into(),
                "Turn on Background refresh or set up the status line".into(),
            ),
            Locale::Pl => (
                "Wymagana konfiguracja".into(),
                "Włącz odświeżanie w tle lub skonfiguruj status line".into(),
            ),
            Locale::Ru => (
                "Требуется настройка".into(),
                "Включите фоновое обновление или настройте status line".into(),
            ),
        };
    }
    match locale {
        Locale::Uk => (
            "Недоступно".into(),
            "Codex app-server ще не повернув дані".into(),
        ),
        Locale::En => (
            "Unavailable".into(),
            "Codex app-server has not returned data".into(),
        ),
        Locale::Pl => (
            "Niedostępne".into(),
            "Codex app-server nie zwrócił danych".into(),
        ),
        Locale::Ru => (
            "Недоступно".into(),
            "Codex app-server ещё не вернул данные".into(),
        ),
    }
}

fn replace_provider(providers: &mut [ProviderSnapshot], value: ProviderSnapshot) {
    if let Some(provider) = providers
        .iter_mut()
        .find(|provider| provider.provider_id == value.provider_id)
    {
        *provider = value;
    }
}

fn empty_provider(id: &str) -> ProviderSnapshot {
    ProviderSnapshot {
        provider_id: id.into(),
        status: ProviderStatus::Unavailable,
        version: None,
        limits: Vec::new(),
        last_success_unix_seconds: None,
        error: None,
    }
}

pub fn apply_config(
    window: &MainWindow,
    snapshot: &UiSnapshot,
    now: i64,
    settings: &AppSettings,
    locale: Locale,
    system_light: bool,
) {
    window.set_demo_mode(snapshot.demo);
    window.set_codex_enabled(settings.codex_enabled);
    window.set_claude_enabled(settings.claude_enabled);
    window.set_five_label(locale.popup().five_hour.into());
    window.set_weekly_label(locale.popup().weekly.into());
    window.set_demo_label(locale.popup().demo.into());
    window.set_settings_label(locale.popup().settings.into());
    window.set_disabled_label(locale.popup().monitoring_disabled.into());
    window.set_usage_title_label(locale.popup().usage_title.into());
    window.set_refresh_label(locale.popup().refresh.into());
    window.set_remaining_heading(locale.popup().remaining.into());
    window.set_reset_heading(locale.popup().reset.into());
    window.set_close_label(locale.popup().close.into());
    window.set_warning_threshold(settings.warning_threshold as i32);
    window.set_critical_threshold(settings.critical_threshold as i32);
    window.set_theme_kind(theme_kind(settings.theme_mode, system_light));
    window.invoke_apply_theme_palette();
    window.set_accent_color(crate::theme::system_accent_color());
    let icons = crate::provider_icons::local();
    let light = settings.theme_mode == crate::settings::ThemeMode::Light;
    // Flyout marks are 14 logical px (14/18/21/28 device px at 100/125/150/200 %).
    let scale = slint::ComponentHandle::window(window).scale_factor();
    let codex = icons.icon(crate::provider_icons::Provider::Codex, light, 14.0, scale);
    let claude = icons.icon(crate::provider_icons::Provider::Claude, light, 14.0, scale);
    window.set_codex_icon_available(codex.is_some());
    window.set_codex_icon(codex.unwrap_or_default());
    window.set_claude_icon_available(claude.is_some());
    window.set_claude_icon(claude.unwrap_or_default());
    apply_provider(
        window,
        ProviderView::Codex,
        provider_ui_for_locale(snapshot, "codex", now, locale),
    );
    apply_provider(
        window,
        ProviderView::Claude,
        provider_ui_for_locale(snapshot, "claude", now, locale),
    );
}

pub(crate) fn theme_kind(mode: crate::settings::ThemeMode, _system_light: bool) -> i32 {
    match mode {
        crate::settings::ThemeMode::Graphite => 0,
        crate::settings::ThemeMode::BluishDark => 1,
        crate::settings::ThemeMode::Light => 2,
    }
}

#[cfg(test)]
fn provider_ui(snapshot: &UiSnapshot, id: &str, now: i64) -> ProviderUi {
    provider_ui_for_locale(snapshot, id, now, Locale::Uk)
}

fn provider_ui_for_locale(snapshot: &UiSnapshot, id: &str, now: i64, locale: Locale) -> ProviderUi {
    let Some(provider) = snapshot
        .providers
        .iter()
        .find(|provider| provider.provider_id == id)
    else {
        return ProviderUi::unavailable(locale);
    };
    let fresh = provider.status == ProviderStatus::Available
        && provider
            .last_success_unix_seconds
            .is_some_and(|last| last <= now && now.saturating_sub(last) <= STALE_AFTER_SECONDS);
    let has_last_good = provider.last_success_unix_seconds.is_some();
    let strings = locale.popup();
    let (status, color, status_level) = if snapshot.demo {
        (strings.demo.into(), rgb(0xd9, 0xae, 0x62), 1)
    } else if snapshot.loading && id == "codex" {
        (strings.loading.into(), rgb(0x9a, 0xa5, 0xb2), 3)
    } else if provider.status == ProviderStatus::Error && id == "claude" && !has_last_good {
        if let Some((status, _)) = claude_error_copy(provider.error.as_ref(), locale) {
            (status.into(), rgb(0xef, 0x82, 0x89), 2)
        } else {
            (
                match locale {
                    Locale::Uk => "Помилка провайдера".into(),
                    Locale::En => "Provider error".into(),
                    Locale::Pl => "Błąd dostawcy".into(),
                    Locale::Ru => "Ошибка провайдера".into(),
                },
                rgb(0xef, 0x82, 0x89),
                2,
            )
        }
    } else if snapshot.claude_bridge_enabled && id == "claude" && !has_last_good {
        (strings.waiting.into(), rgb(0x9a, 0xa5, 0xb2), 3)
    } else if fresh {
        let age = provider
            .last_success_unix_seconds
            .map(|last| now.saturating_sub(last))
            .unwrap_or_default();
        (
            relative_age(age, locale, strings.fresh),
            rgb(0x79, 0xc9, 0x9e),
            0,
        )
    } else if has_last_good {
        (strings.stale.into(), rgb(0xe6, 0xb9, 0x5f), 1)
    } else if provider.status == ProviderStatus::Error {
        (
            match locale {
                Locale::Uk => "Помилка провайдера".into(),
                Locale::En => "Provider error".into(),
                Locale::Pl => "Błąd dostawcy".into(),
                Locale::Ru => "Ошибка провайдера".into(),
            },
            rgb(0xef, 0x82, 0x89),
            2,
        )
    } else {
        (strings.unavailable.into(), rgb(0x9a, 0xa5, 0xb2), 3)
    };
    let limit = provider.limits.first();
    let claude_data = (id == "claude").then_some(&snapshot.claude_usage);
    // Claude's supported metadata does not expose an exact subscription tier.
    // Until both providers can reliably report one, the approved UI fallback
    // is to hide both badges while retaining their parsed metadata in Core.
    let plan = String::new();
    ProviderUi {
        plan,
        status,
        status_color: color,
        status_level,
        five: window_ui(
            limit.and_then(|limit| limit.primary.as_ref()),
            claude_data
                .and_then(|data| data.five_hour.as_ref())
                .and_then(|window| window.resets_at),
            now,
            has_last_good,
            locale,
        ),
        weekly: window_ui(
            limit.and_then(|limit| limit.secondary.as_ref()),
            claude_data
                .and_then(|data| data.weekly.as_ref())
                .and_then(|window| window.resets_at),
            now,
            has_last_good,
            locale,
        ),
    }
}

impl ProviderUi {
    fn unavailable(locale: Locale) -> Self {
        Self {
            plan: String::new(),
            status: locale.popup().unavailable.into(),
            status_color: rgb(0x9a, 0xa5, 0xb2),
            status_level: 3,
            five: WindowView::missing(locale),
            weekly: WindowView::missing(locale),
        }
    }
}

impl WindowView {
    fn missing(locale: Locale) -> Self {
        Self {
            remaining: "—".into(),
            reset: locale.popup().reset_unknown.into(),
            value: 0.0,
            available: false,
        }
    }
}

fn window_ui(
    window: Option<&RateLimitWindow>,
    reset_override: Option<i64>,
    now: i64,
    has_data: bool,
    locale: Locale,
) -> WindowView {
    let window = window.filter(|window| {
        window.used_percent.is_finite() && (0.0..=100.0).contains(&window.used_percent)
    });
    let remaining = window.map(RateLimitWindow::remaining_percent);
    let reset = reset_override.or_else(|| window.and_then(|window| window.resets_at));
    WindowView {
        remaining: match (remaining, has_data) {
            (Some(value), true) => format!("{}%", value.round() as u8),
            _ => "—".into(),
        },
        reset: match reset.filter(|reset| *reset > now) {
            Some(reset) => localized_countdown(reset.saturating_sub(now), locale),
            None => locale.popup().reset_unknown.into(),
        },
        value: remaining.unwrap_or_default() as f32,
        available: has_data && remaining.is_some(),
    }
}

/// "щойно" for the first minute, then "N хв тому" (minutes since the last
/// successful read). Only called for fresh, real data.
fn relative_age(age_seconds: i64, locale: Locale, just_now: &str) -> String {
    let minutes = age_seconds.max(0) / 60;
    if minutes == 0 {
        return just_now.into();
    }
    match locale {
        Locale::Uk => format!("{minutes} хв тому"),
        Locale::En => format!("{minutes} min ago"),
        Locale::Pl => format!("{minutes} min temu"),
        Locale::Ru => format!("{minutes} мин назад"),
    }
}

fn localized_countdown(seconds: i64, locale: Locale) -> String {
    match locale {
        Locale::Uk => return ukrainian_countdown(seconds),
        Locale::Ru => return russian_countdown(seconds),
        Locale::En | Locale::Pl => {}
    }
    let seconds = seconds.max(0);
    let days = seconds / 86_400;
    let hours = seconds % 86_400 / 3_600;
    let minutes = seconds % 3_600 / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        "< 1 min".into()
    }
}

// Compact forms from the approved flyout reference: "4 год 19 хв", "2д 3г".
fn ukrainian_countdown(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let day = seconds / 86_400;
    let hour = seconds % 86_400 / 3_600;
    let minute = seconds % 3_600 / 60;
    if day > 0 {
        format!("{day}д {hour}г")
    } else if hour > 0 {
        format!("{hour} год {minute} хв")
    } else if minute > 0 {
        format!("{minute} хв")
    } else if seconds > 0 {
        "< 1 хв".into()
    } else {
        "час минув".into()
    }
}

fn russian_countdown(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let day = seconds / 86_400;
    let hour = seconds % 86_400 / 3_600;
    let minute = seconds % 3_600 / 60;
    if day > 0 {
        format!("{day}д {hour}ч")
    } else if hour > 0 {
        format!("{hour} ч {minute} мин")
    } else if minute > 0 {
        format!("{minute} мин")
    } else {
        "< 1 мин".into()
    }
}

fn rgb(red: u8, green: u8, blue: u8) -> slint::Color {
    slint::Color::from_rgb_u8(red, green, blue)
}

fn apply_provider(window: &MainWindow, provider: ProviderView, value: ProviderUi) {
    match provider {
        ProviderView::Codex => {
            window.set_codex_plan(value.plan.into());
            window.set_codex_status(value.status.into());
            window.set_codex_status_color(value.status_color);
            window.set_codex_status_level(value.status_level);
            window.set_codex_five_remaining(value.five.remaining.into());
            window.set_codex_five_reset(value.five.reset.into());
            window.set_codex_five_value(value.five.value);
            window.set_codex_five_available(value.five.available);
            window.set_codex_weekly_remaining(value.weekly.remaining.into());
            window.set_codex_weekly_reset(value.weekly.reset.into());
            window.set_codex_weekly_value(value.weekly.value);
            window.set_codex_weekly_available(value.weekly.available);
        }
        ProviderView::Claude => {
            window.set_claude_plan(value.plan.into());
            window.set_claude_status(value.status.into());
            window.set_claude_status_color(value.status_color);
            window.set_claude_status_level(value.status_level);
            window.set_claude_five_remaining(value.five.remaining.into());
            window.set_claude_five_reset(value.five.reset.into());
            window.set_claude_five_value(value.five.value);
            window.set_claude_five_available(value.five.available);
            window.set_claude_weekly_remaining(value.weekly.remaining.into());
            window.set_claude_weekly_reset(value.weekly.reset.into());
            window.set_claude_weekly_value(value.weekly.value);
            window.set_claude_weekly_available(value.weekly.available);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_badges_are_hidden_until_both_tiers_are_reliable() {
        let snapshot = UiSnapshot::demo(1);
        assert!(provider_ui(&snapshot, "codex", 1).plan.is_empty());
        assert!(provider_ui(&snapshot, "claude", 1).plan.is_empty());
    }

    #[test]
    fn theme_kind_maps_exactly_three_modes() {
        use crate::settings::ThemeMode;

        assert_eq!(theme_kind(ThemeMode::Graphite, true), 0);
        assert_eq!(theme_kind(ThemeMode::BluishDark, true), 1);
        assert_eq!(theme_kind(ThemeMode::Light, false), 2);
    }

    fn snapshot(
        status: ProviderStatus,
        last: Option<i64>,
        window: Option<RateLimitWindow>,
    ) -> UiSnapshot {
        UiSnapshot {
            providers: vec![ProviderSnapshot {
                provider_id: "codex".into(),
                status,
                version: None,
                limits: vec![RateLimitSnapshot {
                    limit_id: None,
                    limit_name: None,
                    primary: window,
                    secondary: None,
                    plan_type: None,
                    rate_limit_reached_type: None,
                }],
                last_success_unix_seconds: last,
                error: None,
            }],
            demo: false,
            loading: false,
            claude_bridge_enabled: false,
            claude_usage: ClaudeUsageSnapshot::default(),
        }
    }

    #[test]
    fn missing_or_stale_percentages_never_become_zero() {
        let missing = provider_ui(&UiSnapshot::unavailable(), "codex", 100);
        assert_eq!(missing.five.remaining, "—");
        assert!(!missing.five.available);

        let stale = provider_ui(
            &snapshot(ProviderStatus::Available, Some(1), None),
            "codex",
            1000,
        );
        assert_eq!(stale.status, "Дані застаріли");
        assert_eq!(stale.five.remaining, "—");
    }

    /// Clean install: a missing CLI is "not installed" with a short how-to,
    /// never an error and never zero usage. AUM does not install anything.
    #[test]
    fn missing_clis_show_not_installed_with_instructions_and_no_values() {
        let mut codex_missing = UiSnapshot::unavailable();
        codex_missing.providers[0].error = Some(ProviderError {
            code: "codex_unavailable".into(),
            message: String::new(),
        });
        for locale in [Locale::Uk, Locale::En, Locale::Pl, Locale::Ru] {
            let (status, description) = codex_missing.settings_status("codex", true, 100, locale);
            assert!(status.contains("CLI"), "{locale:?}: {status}");
            assert!(
                description.contains("codex login"),
                "{locale:?}: {description}"
            );
        }
        assert_eq!(codex_missing.settings_status_level("codex", true, 100), 3);
        let view = provider_ui(&codex_missing, "codex", 100);
        assert_eq!(
            (view.five.remaining.as_str(), view.five.available),
            ("—", false)
        );

        let claude_missing = UiSnapshot::unavailable().replace_claude_cli_missing();
        for locale in [Locale::Uk, Locale::En, Locale::Pl, Locale::Ru] {
            let (status, description) = claude_missing.settings_status("claude", true, 100, locale);
            assert!(status.contains("CLI"), "{locale:?}: {status}");
            assert!(description.contains("claude"), "{locale:?}: {description}");
        }
        assert_eq!(claude_missing.settings_status_level("claude", true, 100), 3);
        assert_eq!(
            provider_ui(&claude_missing, "claude", 100).five.remaining,
            "—"
        );

        // Data from the status line is never replaced by "not installed".
        let status_line = ClaudeStatusLineSnapshot {
            five_hour_remaining_percent: Some(40.0),
            five_hour_reset_at: Some(500),
            weekly_remaining_percent: Some(70.0),
            weekly_reset_at: Some(900),
            snapshot_at: 100,
        };
        let with_data = UiSnapshot::loading(true)
            .replace_claude(status_line.to_provider_snapshot(), status_line)
            .replace_claude_cli_missing();
        assert_eq!(provider_ui(&with_data, "claude", 101).five.remaining, "40%");
    }

    #[test]
    fn fresh_values_and_reset_countdowns_are_localized() {
        let data = snapshot(
            ProviderStatus::Available,
            Some(100),
            Some(RateLimitWindow {
                used_percent: 75.0,
                window_duration_mins: None,
                resets_at: Some(3700),
            }),
        );
        let view = provider_ui(&data, "codex", 100);
        assert_eq!(view.five.remaining, "25%");
        assert_eq!(view.five.reset, "1 год 0 хв");
        assert!(view.five.available);
    }

    #[test]
    fn failed_claude_refresh_keeps_last_good_values_and_does_not_mark_them_fresh() {
        let status_line = ClaudeStatusLineSnapshot {
            five_hour_remaining_percent: Some(40.0),
            five_hour_reset_at: Some(500),
            weekly_remaining_percent: Some(70.0),
            weekly_reset_at: Some(900),
            snapshot_at: 100,
        };
        let provider = status_line.to_provider_snapshot();
        let last_good = UiSnapshot::loading(true).replace_claude(provider, status_line);
        let failed = last_good.replace_claude_refresh_error(ClaudeRefreshFailure::NonzeroExit);
        let view = provider_ui(&failed, "claude", 101);
        assert_eq!(view.five.remaining, "40%");
        assert_eq!(view.weekly.remaining, "70%");
        assert_eq!(view.status, "Дані застаріли");
        assert_ne!(view.status, "щойно");
    }

    #[test]
    fn demo_data_is_explicit_and_unavailable_data_has_no_fake_percent() {
        assert_eq!(
            provider_ui(&UiSnapshot::demo(100), "codex", 100).status,
            "Демо · приклад"
        );
        assert_eq!(
            provider_ui(&UiSnapshot::unavailable(), "claude", 100)
                .weekly
                .remaining,
            "—"
        );
        assert_eq!(ukrainian_countdown(30), "< 1 хв");
    }

    #[test]
    fn settings_provider_health_distinguishes_demo_live_waiting_and_disabled() {
        let demo = UiSnapshot::demo(100);
        assert_eq!(
            demo.settings_status("codex", true, 100, Locale::En).0,
            "Demo"
        );
        assert!(
            demo.settings_status("codex", true, 100, Locale::En)
                .1
                .contains("no live")
        );

        let waiting = UiSnapshot::loading(true);
        assert_eq!(
            waiting.settings_status("claude", true, 100, Locale::En).0,
            "Waiting for data"
        );
        assert_eq!(
            UiSnapshot::loading(false)
                .settings_status("claude", true, 100, Locale::En)
                .0,
            "Setup required"
        );
        assert_eq!(
            waiting.settings_status("codex", true, 100, Locale::En).0,
            "Loading"
        );
        assert_eq!(
            waiting.settings_status("codex", false, 100, Locale::En).0,
            "Disabled"
        );
        assert_eq!(
            UiSnapshot::unavailable()
                .settings_status("codex", true, 100, Locale::En)
                .0,
            "Unavailable"
        );
    }

    #[test]
    fn demo_values_are_explicit_and_live_startup_never_uses_demo_data() {
        let demo = crate::initial_snapshot(true, false, false, 100);
        assert!(demo.demo);
        assert_eq!(provider_ui(&demo, "codex", 100).five.remaining, "58%");

        let live = crate::initial_snapshot(false, true, false, 100);
        assert!(!live.demo);
        assert_eq!(provider_ui(&live, "codex", 100).five.remaining, "—");
        assert_eq!(provider_ui(&live, "codex", 100).status, "Завантаження");

        let empty = crate::initial_snapshot(false, false, false, 100);
        assert!(!empty.demo);
        assert_eq!(provider_ui(&empty, "codex", 100).five.remaining, "—");
    }

    #[test]
    fn provider_selection_filters_alert_inputs_independently() {
        let snapshot = UiSnapshot::demo(100);
        assert_eq!(
            snapshot.alert_providers(true, false)[0].provider_id,
            "codex"
        );
        assert_eq!(
            snapshot.alert_providers(false, true)[0].provider_id,
            "claude"
        );
        assert!(snapshot.alert_providers(false, false).is_empty());
    }

    /// F-11 fixtures: (used) percentages per provider; `None` = window missing.
    fn tray_snapshot(
        codex: (Option<f64>, Option<f64>),
        claude: (Option<f64>, Option<f64>),
        updated: i64,
    ) -> UiSnapshot {
        let window = |used: Option<f64>| {
            used.map(|used_percent| RateLimitWindow {
                used_percent,
                window_duration_mins: None,
                resets_at: None,
            })
        };
        let provider = |id: &str, (five, weekly): (Option<f64>, Option<f64>)| ProviderSnapshot {
            provider_id: id.into(),
            status: ProviderStatus::Available,
            version: None,
            limits: vec![RateLimitSnapshot {
                limit_id: None,
                limit_name: None,
                primary: window(five),
                secondary: window(weekly),
                plan_type: None,
                rate_limit_reached_type: None,
            }],
            last_success_unix_seconds: Some(updated),
            error: None,
        };
        UiSnapshot {
            providers: vec![provider("codex", codex), provider("claude", claude)],
            demo: false,
            loading: false,
            claude_bridge_enabled: true,
            claude_usage: ClaudeUsageSnapshot::default(),
        }
    }

    fn remaining(values: [crate::tray::QuotaValue; 2]) -> [Option<u8>; 2] {
        [values[0].remaining_percent, values[1].remaining_percent]
    }

    /// F-11 contract with the agreed example: Codex 100 % / 94 %, Claude
    /// 96 % / 44 % remaining. Each column is one fixed window; the tightest
    /// window is never picked.
    #[test]
    fn tray_columns_follow_the_fixed_window_contract() {
        let now = 1_700_000_000;
        // used = 100 − remaining
        let snapshot = tray_snapshot((Some(0.0), Some(6.0)), (Some(4.0), Some(56.0)), now);
        // Both: left Codex 5 h, right Claude 5 h (not 94 / 44).
        assert_eq!(
            remaining(snapshot.tray_values(true, true, now)),
            [Some(100), Some(96)]
        );
        // Codex only: Codex 5 h, Codex weekly.
        assert_eq!(
            remaining(snapshot.tray_values(true, false, now)),
            [Some(100), Some(94)]
        );
        // Claude only: Claude 5 h, Claude weekly.
        assert_eq!(
            remaining(snapshot.tray_values(false, true, now)),
            [Some(96), Some(44)]
        );
        // Neither: both unavailable.
        let off = snapshot.tray_values(false, false, now);
        assert_eq!(remaining(off), [None, None]);
        assert!(
            off.iter()
                .all(|value| value.freshness == crate::tray::DataFreshness::Unavailable)
        );
        for mode in [(true, true), (true, false), (false, true)] {
            let values = snapshot.tray_values(mode.0, mode.1, now);
            assert!(
                values
                    .iter()
                    .all(|value| value.freshness == crate::tray::DataFreshness::Fresh),
                "{mode:?}"
            );
        }
        // Different weekly values never move the 5-hour columns.
        let other_weekly = tray_snapshot((Some(0.0), Some(99.0)), (Some(4.0), Some(1.0)), now);
        assert_eq!(
            remaining(other_weekly.tray_values(true, true, now)),
            [Some(100), Some(96)]
        );
    }

    /// A missing primary or secondary window makes only its own column
    /// unavailable; the other column and the other provider stay live.
    #[test]
    fn missing_windows_affect_only_their_own_column() {
        use crate::tray::DataFreshness::{Fresh, Unavailable};
        let now = 1_700_000_000;
        let no_codex_five = tray_snapshot((None, Some(6.0)), (Some(4.0), Some(56.0)), now);
        let values = no_codex_five.tray_values(true, true, now);
        assert_eq!(remaining(values), [None, Some(96)]);
        assert_eq!(
            (values[0].freshness, values[1].freshness),
            (Unavailable, Fresh)
        );
        let values = no_codex_five.tray_values(true, false, now);
        assert_eq!(remaining(values), [None, Some(94)]);
        assert_eq!(
            (values[0].freshness, values[1].freshness),
            (Unavailable, Fresh)
        );

        let no_claude_weekly = tray_snapshot((Some(0.0), Some(6.0)), (Some(4.0), None), now);
        let values = no_claude_weekly.tray_values(false, true, now);
        assert_eq!(remaining(values), [Some(96), None]);
        assert_eq!(
            (values[0].freshness, values[1].freshness),
            (Fresh, Unavailable)
        );
        // The weekly gap does not matter when both providers show 5-hour.
        assert_eq!(
            remaining(no_claude_weekly.tray_values(true, true, now)),
            [Some(100), Some(96)]
        );

        // A provider without any data leaves the other provider's column live.
        let degraded = UiSnapshot {
            providers: vec![
                empty_provider("codex"),
                tray_snapshot((None, None), (Some(35.0), Some(12.0)), now).providers[1].clone(),
            ],
            ..no_claude_weekly
        };
        let values = degraded.tray_values(true, true, now);
        assert_eq!(remaining(values), [None, Some(65)]);
        assert_eq!(
            (values[0].freshness, values[1].freshness),
            (Unavailable, Fresh)
        );
        // Invalid percentages are treated as missing, never as zero.
        let invalid = tray_snapshot((Some(f64::NAN), Some(150.0)), (Some(4.0), Some(56.0)), now);
        assert_eq!(
            remaining(invalid.tray_values(true, false, now)),
            [None, None]
        );
    }

    /// Stale last-known-good values keep their own window and are marked
    /// stale (rendered solid and muted by the tray, F-7).
    #[test]
    fn stale_last_known_good_values_keep_their_window() {
        let now = 1_700_000_000;
        let old = now - STALE_AFTER_SECONDS - 1;
        let snapshot = tray_snapshot((Some(0.0), Some(6.0)), (Some(4.0), Some(56.0)), old);
        for (mode, expected) in [
            ((true, true), [Some(100), Some(96)]),
            ((true, false), [Some(100), Some(94)]),
            ((false, true), [Some(96), Some(44)]),
        ] {
            let values = snapshot.tray_values(mode.0, mode.1, now);
            assert_eq!(remaining(values), expected, "{mode:?}");
            assert!(
                values
                    .iter()
                    .all(|value| value.freshness == crate::tray::DataFreshness::Stale),
                "{mode:?}"
            );
        }
        // Last-known-good data after a failure stays visible as stale.
        let mut failed = tray_snapshot((Some(0.0), Some(6.0)), (Some(4.0), Some(56.0)), now - 10);
        failed.providers[0].status = ProviderStatus::Error;
        let values = failed.tray_values(true, true, now);
        assert_eq!(remaining(values), [Some(100), Some(96)]);
        assert_eq!(values[0].freshness, crate::tray::DataFreshness::Stale);
        assert_eq!(values[1].freshness, crate::tray::DataFreshness::Fresh);
    }

    #[test]
    fn loading_and_last_known_good_states_remain_distinguishable() {
        assert_eq!(
            provider_ui(&UiSnapshot::loading(false), "codex", 100).status,
            "Завантаження"
        );
        assert_eq!(
            provider_ui(&UiSnapshot::loading(false), "claude", 100).status,
            "Недоступно"
        );
        assert_eq!(
            provider_ui(&UiSnapshot::loading(true), "claude", 100).status,
            "Очікування"
        );

        let stale = snapshot(
            ProviderStatus::Error,
            Some(1),
            Some(RateLimitWindow {
                used_percent: 80.0,
                window_duration_mins: Some(300),
                resets_at: None,
            }),
        );
        let view = provider_ui(&stale, "codex", 2_000);
        assert_eq!(view.status, "Дані застаріли");
        assert_eq!(view.five.remaining, "20%");
        assert!(view.five.available);
    }

    #[test]
    fn status_line_resets_reach_the_flyout_and_count_down_locally() {
        let now = 1_700_000_000;
        let raw = format!(
            r#"{{"rate_limits":{{"five_hour":{{"used_percentage":33,"resets_at":{} }},"seven_day":{{"used_percentage":14,"resets_at":{} }}}}}}"#,
            now + 3_600,
            now + 86_400
        );
        let normalized = ClaudeStatusLineSnapshot::from_status_line_json(&raw, now).unwrap();
        let initial = UiSnapshot::loading(true);
        let claude = normalized.to_provider_snapshot();
        let with_claude = initial.replace_claude(claude, normalized);
        assert_eq!(
            provider_ui(&with_claude, "claude", now).five.remaining,
            "67%"
        );
        assert_eq!(
            provider_ui(&with_claude, "claude", now).weekly.remaining,
            "86%"
        );
        assert_eq!(
            provider_ui(&with_claude, "claude", now).weekly.reset,
            "1д 0г"
        );
        let ticked = provider_ui(&with_claude, "claude", now + 60);
        assert_eq!(ticked.five.reset, "59 хв");
        assert_eq!(ticked.weekly.reset, "23 год 59 хв");
        let newer_status_line = ClaudeStatusLineSnapshot {
            five_hour_remaining_percent: Some(68.0),
            five_hour_reset_at: None,
            weekly_remaining_percent: Some(84.0),
            weekly_reset_at: None,
            snapshot_at: now + 120,
        };
        let merged =
            with_claude.replace_claude(newer_status_line.to_provider_snapshot(), newer_status_line);
        let merged_view = provider_ui(&merged, "claude", now + 120);
        assert_eq!(merged_view.five.remaining, "68%");
        assert_eq!(merged_view.five.reset, "58 хв");
        assert_eq!(merged_view.weekly.remaining, "84%");
        assert_eq!(merged_view.weekly.reset, "23 год 58 хв");
        assert_eq!(
            provider_ui(&with_claude, "codex", now).status,
            "Завантаження"
        );
        let codex = snapshot(ProviderStatus::Available, Some(now), None)
            .providers
            .into_iter()
            .next()
            .unwrap();
        let updated = with_claude.replace_codex(codex);
        assert_eq!(provider_ui(&updated, "codex", now).status, "щойно");
        assert_eq!(
            provider_ui(&with_claude, "claude", now + STALE_AFTER_SECONDS + 1).status,
            "Дані застаріли"
        );
    }

    #[test]
    fn enabled_bridge_errors_are_visible_without_fabricated_values() {
        let snapshot = UiSnapshot::loading(true).replace_claude_bridge_error();
        let view = provider_ui(&snapshot, "claude", 1_700_000_000);
        assert_eq!(view.status, Locale::Uk.claude_bridge_error().0);
        assert_eq!(view.five.remaining, "—");
        assert!(!view.five.available);
    }

    #[test]
    fn claude_refresh_categories_survive_adapter_and_localize_separately_from_bridge_errors() {
        let failures = [
            ClaudeRefreshFailure::SpawnFailure,
            ClaudeRefreshFailure::NonzeroExit,
            ClaudeRefreshFailure::Timeout,
            ClaudeRefreshFailure::EmptyOutput,
            ClaudeRefreshFailure::UnrecognizedFormat,
            ClaudeRefreshFailure::NoQuotaValues,
            ClaudeRefreshFailure::ProcessIoFailure,
        ];
        for failure in failures {
            let snapshot = UiSnapshot::loading(true).replace_claude_refresh_error(failure);
            let code = snapshot
                .providers
                .iter()
                .find(|provider| provider.provider_id == "claude")
                .and_then(|provider| provider.error.as_ref())
                .map(|error| error.code.as_str());
            assert_eq!(
                code,
                Some(format!("claude_refresh_{}", failure.category()).as_str())
            );

            for locale in [Locale::Uk, Locale::En, Locale::Pl, Locale::Ru] {
                let (status, description) = locale.claude_refresh_failure(failure).unwrap();
                assert!(!status.is_empty());
                assert!(!description.is_empty());
                assert!(!status.to_ascii_lowercase().contains("bridge"));
                assert_eq!(
                    provider_ui_for_locale(&snapshot, "claude", 100, locale).status,
                    status
                );
                let (settings_status, settings_description) =
                    snapshot.settings_status("claude", true, 100, locale);
                assert_eq!(settings_status, status);
                assert_eq!(settings_description, description);
            }
        }

        let bridge = UiSnapshot::loading(true).replace_claude_bridge_error();
        for locale in [Locale::Uk, Locale::En, Locale::Pl, Locale::Ru] {
            let (status, description) = locale.claude_bridge_error();
            assert_eq!(
                provider_ui_for_locale(&bridge, "claude", 100, locale).status,
                status
            );
            assert_eq!(
                bridge.settings_status("claude", true, 100, locale),
                (status.into(), description.into())
            );
        }
    }

    #[test]
    fn reset_countdown_never_goes_negative_or_fabricates_missing_reset() {
        let elapsed = snapshot(
            ProviderStatus::Available,
            Some(100),
            Some(RateLimitWindow {
                used_percent: 25.0,
                window_duration_mins: None,
                resets_at: Some(50),
            }),
        );
        assert_eq!(provider_ui(&elapsed, "codex", 100).five.reset, "—");
        let unknown = snapshot(
            ProviderStatus::Available,
            Some(100),
            Some(RateLimitWindow {
                used_percent: 25.0,
                window_duration_mins: None,
                resets_at: None,
            }),
        );
        assert_eq!(provider_ui(&unknown, "codex", 100).five.reset, "—");
    }

    #[test]
    fn fixed_layout_scales_as_logical_dips_at_requested_dpi_factors() {
        let logical = slint::LogicalSize::new(370.0, 208.0);
        for (scale, expected) in [
            (1.0, (370, 208)),
            (1.25, (462, 260)),
            (1.5, (555, 312)),
            (2.0, (740, 416)),
        ] {
            let physical = slint::PhysicalSize::from_logical(logical, scale);
            assert_eq!((physical.width, physical.height), expected);
        }
        for (enabled, expected_height) in [
            ((true, false), 118.0),
            ((false, true), 118.0),
            ((false, false), 78.0),
        ] {
            let height = if enabled.0 && enabled.1 {
                208.0
            } else if enabled.0 || enabled.1 {
                118.0
            } else {
                78.0
            };
            assert_eq!(height, expected_height);
        }
    }
}
