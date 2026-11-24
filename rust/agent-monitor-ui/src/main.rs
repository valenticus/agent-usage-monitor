// Release builds are GUI-subsystem apps (no console window). Debug builds
// keep the console for the diagnostic log lines.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adapter;
mod aum_icon;
mod autostart;
mod claude_refresher;
mod claude_statusline;
mod flyout;
mod localization;
mod notifications;
mod packaged;
mod provider_icons;
mod settings;
mod theme;
mod tray;
mod tray_menu;
mod update_checker;
mod winrt;

slint::include_modules!();

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use agent_usage_monitor_core::{
    AlertEngine, AlertEvent, AlertSettings, AlertStateStore, AlertWindow, ClaudeStatusLineSnapshot,
    CodexAppServerClient, CodexClientConfig, CodexRateLimitProvider, ProviderError, ProviderState,
    ProviderStatus, RateLimitProvider, run_claude_status_pipe,
};
use localization::Locale;
use settings::{AppSettings, Language, ThemeMode};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use update_checker::{Distribution, UiState, UpdateStatus};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Delay after a Windows resume before the background refresh, so networking
/// is usually back; repeated resume notifications restart (coalesce) it.
#[cfg(windows)]
const RESUME_REFRESH_DELAY: Duration = Duration::from_secs(10);
type SharedSettings = Arc<Mutex<AppSettings>>;
type SharedLocale = Arc<Mutex<Locale>>;
type SharedSnapshot = Arc<Mutex<adapter::UiSnapshot>>;
type SharedAlerts = Arc<Mutex<AlertRuntime>>;
static NEXT_REFRESH_ID: AtomicU64 = AtomicU64::new(1);
static UPDATE_STATUS: std::sync::Mutex<UpdateStatus> = std::sync::Mutex::new(UpdateStatus::Idle);
static SETTINGS_MUTATION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ManualRefreshRequest {
    pub(crate) id: u64,
    pub(crate) origin: RefreshOrigin,
}

/// Why a refresh was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefreshOrigin {
    /// Tray menu or flyout refresh button.
    Manual,
    /// The delayed background refresh after a Windows resume.
    Resume,
}

/// A resume refresh is redundant when Codex already refreshed successfully
/// this recently: after sleep, the overdue 5-minute tick fires as soon as the
/// worker wakes, which is about when the delayed resume request arrives.
const RESUME_COALESCE_SECONDS: i64 = 30;

/// Skips a resume-origin refresh (Codex or Claude CLI) that would only repeat
/// a successful refresh from the last `RESUME_COALESCE_SECONDS`. Manual
/// refreshes always run; a failed refresh (no recent success) never
/// suppresses the retry.
pub(crate) fn skip_redundant_resume_refresh(
    origin: RefreshOrigin,
    last_success: Option<i64>,
    now: i64,
) -> bool {
    origin == RefreshOrigin::Resume
        && last_success
            .is_some_and(|last| last <= now && now.saturating_sub(last) < RESUME_COALESCE_SECONDS)
}

struct AlertRuntime {
    engine: AlertEngine,
    /// `None` without persistent storage: alert state stays in memory.
    state_path: Option<std::path::PathBuf>,
}

impl AlertRuntime {
    fn new(settings: &AppSettings, state_path: Option<std::path::PathBuf>) -> Self {
        let state = state_path
            .as_deref()
            .and_then(|path| AlertStateStore::load(path).ok())
            .unwrap_or_default();
        let engine = AlertEngine::new(alert_settings(settings), state)
            .expect("settings schema validation ensures valid alert configuration");
        Self { engine, state_path }
    }

    fn refresh(
        &mut self,
        snapshot: &adapter::UiSnapshot,
        settings: &AppSettings,
        now: i64,
    ) -> Vec<AlertEvent> {
        let _ = self.engine.set_settings(alert_settings(settings));
        let providers = snapshot.alert_providers(settings.codex_enabled, settings.claude_enabled);
        let events = self.engine.refresh(&providers, now);
        if let Some(path) = &self.state_path {
            let _ = AlertStateStore::save(path, self.engine.state());
        }
        events
    }
}

fn alert_settings(settings: &AppSettings) -> AlertSettings {
    AlertSettings {
        enabled: settings.alerts_enabled,
        warning_threshold: settings.warning_threshold,
        critical_threshold: settings.critical_threshold,
        exhausted_threshold: 0,
        exhausted_alerts_enabled: settings.exhausted_alerts_enabled,
        include_weekly: settings.include_weekly,
        popup_duration_seconds: settings.popup_duration_seconds,
        stale_after_seconds: 900,
    }
}

fn alert_notification_copy(event: &AlertEvent, locale: Locale) -> (String, String) {
    use agent_usage_monitor_core::AlertSeverity;
    let provider = if event.key.provider_id == "claude" {
        "Claude"
    } else {
        "Codex"
    };
    let window = match (locale, event.key.window) {
        (Locale::Uk, AlertWindow::FiveHour) => "5 год",
        (Locale::Uk, AlertWindow::Weekly) => "тижневий ліміт",
        (Locale::En, AlertWindow::FiveHour) => "5-hour limit",
        (Locale::En, AlertWindow::Weekly) => "weekly limit",
        (Locale::Pl, AlertWindow::FiveHour) => "limit 5-godzinny",
        (Locale::Pl, AlertWindow::Weekly) => "limit tygodniowy",
        (Locale::Ru, AlertWindow::FiveHour) => "лимит на 5 часов",
        (Locale::Ru, AlertWindow::Weekly) => "недельный лимит",
    };
    let severity = match (locale, event.severity) {
        (Locale::Uk, AlertSeverity::Warning) => "попередження",
        (Locale::Uk, AlertSeverity::Critical) => "критичний рівень",
        (Locale::Uk, AlertSeverity::Exhausted) => "вичерпано",
        (Locale::En, AlertSeverity::Warning) => "warning",
        (Locale::En, AlertSeverity::Critical) => "critical",
        (Locale::En, AlertSeverity::Exhausted) => "exhausted",
        (Locale::Pl, AlertSeverity::Warning) => "ostrzeżenie",
        (Locale::Pl, AlertSeverity::Critical) => "poziom krytyczny",
        (Locale::Pl, AlertSeverity::Exhausted) => "wyczerpany",
        (Locale::Ru, AlertSeverity::Warning) => "предупреждение",
        (Locale::Ru, AlertSeverity::Critical) => "критический уровень",
        (Locale::Ru, AlertSeverity::Exhausted) => "исчерпан",
    };
    let message = match locale {
        Locale::Uk => format!("Залишилося {}%", event.remaining_percent),
        Locale::En => format!("{}% remaining", event.remaining_percent),
        Locale::Pl => format!("Pozostało {}%", event.remaining_percent),
        Locale::Ru => format!("Осталось {}%", event.remaining_percent),
    };
    (format!("{provider} · {window} · {severity}"), message)
}

fn deliver_alerts(notifier: &Option<tray::TrayNotifier>, events: Vec<AlertEvent>, locale: Locale) {
    let Some(notifier) = notifier else {
        return;
    };
    for event in events {
        let (title, message) = alert_notification_copy(&event, locale);
        notifier.notify(&title, &message, event.popup_duration_seconds);
    }
}

fn queue_manual_refresh(
    provider: &str,
    sender: &mpsc::Sender<ManualRefreshRequest>,
    request: ManualRefreshRequest,
) -> bool {
    match sender.try_send(request) {
        Ok(()) => {
            eprintln!("REFRESH: queued id={} provider={provider}", request.id);
            true
        }
        Err(mpsc::error::TrySendError::Full(_)) => {
            eprintln!("REFRESH: coalesced id={} provider={provider}", request.id);
            true
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            eprintln!(
                "REFRESH: skipped id={} provider={provider} reason=worker_closed",
                request.id
            );
            false
        }
    }
}

fn initial_snapshot(
    demo_mode: bool,
    live_mode: bool,
    bridge: bool,
    now: i64,
) -> adapter::UiSnapshot {
    if demo_mode {
        adapter::UiSnapshot::demo(now)
    } else if live_mode {
        adapter::UiSnapshot::loading(bridge)
    } else {
        adapter::UiSnapshot::unavailable()
    }
}

#[derive(Clone)]
struct WorkerUiContext {
    window: slint::Weak<MainWindow>,
    settings_window: slint::Weak<SettingsWindow>,
    current: SharedSnapshot,
    config: SharedSettings,
    locale: SharedLocale,
    alerts: SharedAlerts,
    notifier: Option<tray::TrayNotifier>,
}

fn main() -> Result<(), slint::PlatformError> {
    #[cfg(windows)]
    let _instance_guard = match SingleInstance::acquire() {
        Ok(Some(guard)) => guard,
        Ok(None) => return Ok(()),
        Err(()) => {
            eprintln!("Could not establish the single-instance guard.");
            return Ok(());
        }
    };
    // App identity for toasts (sender name + static AUM icon); per-user only.
    #[cfg(windows)]
    if let Err(error) = notifications::register_identity() {
        eprintln!("NOTIFY: app identity registration failed: {error}");
    }
    let args: Vec<String> = std::env::args().collect();
    #[cfg(debug_assertions)]
    if let Some(state) = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--preview-startup="))
    {
        let _ = PREVIEW_STARTUP.set(state.to_owned());
    }
    let demo_mode = args.iter().any(|arg| arg == "--demo");
    let live_mode = args.iter().any(|arg| arg == "--live");
    let empty_mode = args.iter().any(|arg| arg == "--empty");
    let claude_bridge_enabled = args.iter().any(|arg| arg == "--enable-claude-bridge");
    if usize::from(demo_mode) + usize::from(live_mode) + usize::from(empty_mode) > 1
        || (claude_bridge_enabled && (demo_mode || empty_mode))
    {
        eprintln!("Choose only one mode: --live, --demo, or --empty.");
        return Ok(());
    }
    let use_live = live_mode || (!demo_mode && !empty_mode);
    // Empty path = no persistent storage (Store data folder unavailable or
    // unknown identity): defaults in memory, every save refused and reported
    // in Settings. Never a %TEMP% or EXE-folder fallback.
    let data_directory = settings::settings_directory();
    let settings_path = settings::settings_path().unwrap_or_default();
    // The legacy "Agent Monitor" folder belongs to old EXE builds only. The
    // Store build never migrates automatically; it offers an explicit import.
    if let Some(directory) = data_directory
        .as_ref()
        .filter(|_| packaged::is_confirmed_unpackaged())
        && settings::migrate_legacy_data(&settings::legacy_settings_directory(), directory).is_err()
    {
        eprintln!("Settings migration could not complete; existing data was retained.");
    }
    if let Some(directory) = data_directory.as_ref().filter(|_| packaged::is_packaged()) {
        // Checked once (read-only) at launch; the row also disappears as soon
        // as the Store build has its own settings file.
        let _ = STORE_IMPORT_SOURCE_VALID.set(settings::import_available(
            &settings::unpackaged_settings_directory(),
            directory,
        ));
    }
    let config: SharedSettings = Arc::new(Mutex::new(
        settings::load(&settings_path).unwrap_or_default(),
    ));
    reconcile_autostart(&config, &settings_path, claude_bridge_enabled);
    // Debug-only visual review aid: override the theme in memory (never saved).
    #[cfg(debug_assertions)]
    if let Some(theme) = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--preview-theme="))
    {
        if let Ok(mut current) = config.lock() {
            current.theme_mode = match theme {
                "light" => ThemeMode::Light,
                "bluish" => ThemeMode::BluishDark,
                _ => ThemeMode::Graphite,
            };
        }
    }
    #[cfg(debug_assertions)]
    if let Some(language) = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--preview-language="))
    {
        if let Ok(mut current) = config.lock() {
            current.language = match language {
                "en" => Language::English,
                "pl" => Language::Polish,
                "ru" => Language::Russian,
                _ => Language::Ukrainian,
            };
        }
    }
    let locale: SharedLocale = Arc::new(Mutex::new(resolve_locale(&lock_settings(&config))));
    let initial_snapshot = initial_snapshot(
        demo_mode,
        use_live,
        claude_bridge_wanted(claude_bridge_enabled, &lock_settings(&config)),
        now_unix(),
    );
    let current: SharedSnapshot = Arc::new(Mutex::new(initial_snapshot.clone()));
    let alerts: SharedAlerts = Arc::new(Mutex::new(AlertRuntime::new(
        &lock_settings(&config),
        data_directory
            .as_ref()
            .map(|directory| directory.join("alert-state.json")),
    )));

    // The flyout is shown without activation only for temporary tray previews.
    // Explicit tray Open and Settings actions activate their windows normally.
    // The tray menu (also frameless) needs keyboard focus.
    slint::BackendSelector::new()
        .with_winit_window_attributes_hook(|attributes| {
            if attributes.title == "AgentUsageMonitor" {
                attributes.with_active(false)
            } else {
                attributes
            }
        })
        .select()?;

    let window = MainWindow::new()?;
    let settings_window = SettingsWindow::new()?;
    window.set_refresh_enabled(use_live);
    settings_window.set_window_icon(theme::aum_icon_image(64));
    adapter::apply_config(
        &window,
        &initial_snapshot,
        now_unix(),
        &lock_settings(&config),
        lock_locale(&locale),
        system_light(&lock_settings(&config)),
    );
    update_settings_window(
        &settings_window,
        &lock_settings(&config),
        lock_locale(&locale),
        system_light(&lock_settings(&config)),
        &initial_snapshot,
        now_unix(),
    );
    let update_state = Arc::new(Mutex::new(UiState::default()));
    let update_checking = Arc::new(AtomicBool::new(false));
    let update_distribution = Distribution::current();

    let (settings_sender, settings_receiver) = watch::channel(lock_settings(&config));
    window.on_open_settings({
        let panel = settings_window.as_weak();
        move || {
            if let Some(panel) = panel.upgrade() {
                show_settings_centered(&panel);
            }
        }
    });
    let (refresh_sender, refresh_receiver) = mpsc::channel(1);
    let (claude_refresh_sender, claude_refresh_receiver) = mpsc::channel(1);
    // Dispatch one bounded request to each opted-in provider independently.
    // Full channels mean an equivalent request is already pending, so clicks
    // coalesce instead of starting duplicate CLI processes.
    let request_refresh: Arc<dyn Fn(RefreshOrigin) -> bool + Send + Sync> = {
        let codex_sender = refresh_sender.clone();
        let claude_sender = claude_refresh_sender.clone();
        let config = config.clone();
        Arc::new(move |origin| {
            if !use_live {
                eprintln!("REFRESH: skipped reason=not_live");
                return false;
            }
            let settings = lock_settings(&config);
            let codex_enabled = settings.codex_enabled;
            let claude_enabled = settings.claude_enabled;
            let claude_cli_enabled = settings.claude_cli_refresh_enabled;
            let id = NEXT_REFRESH_ID.fetch_add(1, Ordering::Relaxed);
            let request = ManualRefreshRequest { id, origin };
            eprintln!("REFRESH: requested id={id} origin={origin:?}");
            let codex = if codex_enabled {
                queue_manual_refresh("codex", &codex_sender, request)
            } else {
                eprintln!("REFRESH: skipped id={id} provider=codex reason=disabled");
                false
            };
            let claude = if !claude_enabled {
                eprintln!("REFRESH: skipped id={id} provider=claude reason=disabled");
                false
            } else if !claude_cli_enabled {
                eprintln!("REFRESH: skipped id={id} provider=claude reason=cli_opt_in_disabled");
                false
            } else {
                queue_manual_refresh("claude", &claude_sender, request)
            };
            codex || claude
        })
    };
    window.on_refresh_requested({
        let dispatcher = request_refresh.clone();
        move || {
            let _ = dispatcher(RefreshOrigin::Manual);
        }
    });
    settings_window.window().on_close_requested({
        let panel = settings_window.as_weak();
        move || {
            if let Some(panel) = panel.upgrade() {
                let _ = panel.hide();
            }
            slint::CloseRequestResponse::HideWindow
        }
    });
    settings_window.on_select_tab({
        let panel = settings_window.as_weak();
        move |tab| {
            if valid_settings_tab(tab) {
                if let Some(panel) = panel.upgrade() {
                    panel.set_selected_tab(tab);
                }
            }
        }
    });
    settings_window.on_theme_selected({
        let values = callback_context(
            &config,
            &settings_path,
            &locale,
            &settings_sender,
            &window,
            &settings_window,
            &current,
        );
        move |value| {
            let locale = lock_locale(&values.2);
            save_setting_change(&values, |settings| set_theme(settings, &value, locale));
        }
    });
    settings_window.on_language_selected({
        let values = callback_context(
            &config,
            &settings_path,
            &locale,
            &settings_sender,
            &window,
            &settings_window,
            &current,
        );
        move |value| {
            let locale = lock_locale(&values.2);
            save_setting_change(&values, |settings| set_language(settings, &value, locale));
        }
    });
    settings_window.on_toggle_codex({
        let values = callback_context(
            &config,
            &settings_path,
            &locale,
            &settings_sender,
            &window,
            &settings_window,
            &current,
        );
        move || save_setting_change(&values, toggle_codex)
    });
    settings_window.on_toggle_claude({
        let values = callback_context(
            &config,
            &settings_path,
            &locale,
            &settings_sender,
            &window,
            &settings_window,
            &current,
        );
        move || save_setting_change(&values, toggle_claude)
    });
    settings_window.on_toggle_claude_cli_refresh({
        let values = callback_context(
            &config,
            &settings_path,
            &locale,
            &settings_sender,
            &window,
            &settings_window,
            &current,
        );
        move || {
            save_setting_change(&values, |settings| {
                settings.claude_cli_refresh_enabled = !settings.claude_cli_refresh_enabled
            })
        }
    });
    settings_window.on_toggle_claude_bridge_listener({
        let values = callback_context(
            &config,
            &settings_path,
            &locale,
            &settings_sender,
            &window,
            &settings_window,
            &current,
        );
        move || toggle_claude_bridge_listener(&values, claude_bridge_enabled)
    });
    settings_window.on_copy_statusline_setting({
        let panel = settings_window.as_weak();
        move || {
            if let Some(panel) = panel.upgrade() {
                copy_statusline_setting(&panel);
            }
        }
    });
    settings_window.on_import_exe_settings({
        let values = callback_context(
            &config,
            &settings_path,
            &locale,
            &settings_sender,
            &window,
            &settings_window,
            &current,
        );
        move || import_exe_settings(&values)
    });
    // MSIX: StartupTask results arrive on the controller's worker thread and
    // are applied on the UI thread. At launch the task is only read.
    if packaged::is_packaged() {
        let values = callback_context(
            &config,
            &settings_path,
            &locale,
            &settings_sender,
            &window,
            &settings_window,
            &current,
        );
        let values = Mutex::new(values);
        let controller = autostart::packaged_controller();
        controller.set_listener(move |request, result| {
            let Ok(values) = values.lock().map(|values| values.clone()) else {
                return;
            };
            let result = result.clone();
            let _ = slint::invoke_from_event_loop(move || {
                apply_packaged_startup_result(&values, request, result);
            });
        });
        if let Some(request) = launch_startup_request(packaged::state()) {
            controller.submit(request);
        }
    }
    settings_window.on_toggle_startup({
        let values = callback_context(
            &config,
            &settings_path,
            &locale,
            &settings_sender,
            &window,
            &settings_window,
            &current,
        );
        move || toggle_autostart(&values)
    });
    settings_window.on_toggle_alerts({
        let values = callback_context(
            &config,
            &settings_path,
            &locale,
            &settings_sender,
            &window,
            &settings_window,
            &current,
        );
        move || {
            save_setting_change(&values, |settings| {
                settings.alerts_enabled = !settings.alerts_enabled
            })
        }
    });
    settings_window.on_warning_selected({
        let values = callback_context(
            &config,
            &settings_path,
            &locale,
            &settings_sender,
            &window,
            &settings_window,
            &current,
        );
        move |value| {
            if let Ok(threshold) = value.trim_end_matches('%').parse::<u8>() {
                save_setting_change(&values, |settings| {
                    set_warning_threshold(settings, threshold)
                });
            }
        }
    });
    settings_window.on_critical_selected({
        let values = callback_context(
            &config,
            &settings_path,
            &locale,
            &settings_sender,
            &window,
            &settings_window,
            &current,
        );
        move |value| {
            if let Ok(threshold) = value.trim_end_matches('%').parse::<u8>() {
                save_setting_change(&values, |settings| {
                    set_critical_threshold(settings, threshold)
                });
            }
        }
    });
    settings_window.on_toggle_exhausted_alerts({
        let values = callback_context(
            &config,
            &settings_path,
            &locale,
            &settings_sender,
            &window,
            &settings_window,
            &current,
        );
        move || {
            save_setting_change(&values, |settings| {
                settings.exhausted_alerts_enabled = !settings.exhausted_alerts_enabled
            })
        }
    });
    settings_window.on_toggle_weekly({
        let values = callback_context(
            &config,
            &settings_path,
            &locale,
            &settings_sender,
            &window,
            &settings_window,
            &current,
        );
        move || {
            save_setting_change(&values, |settings| {
                settings.include_weekly = !settings.include_weekly
            })
        }
    });
    settings_window.on_duration_selected({
        let values = callback_context(
            &config,
            &settings_path,
            &locale,
            &settings_sender,
            &window,
            &settings_window,
            &current,
        );
        move |value| {
            if let Some(seconds) = value
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u16>().ok())
            {
                save_setting_change(&values, |settings| {
                    set_notification_duration(settings, seconds);
                });
            }
        }
    });
    settings_window.on_open_repository(open_repository);
    settings_window.on_open_issues(open_issues);
    settings_window.set_update_distribution_label(
        update_checker::distribution_label(update_distribution, lock_locale(&locale)).into(),
    );
    settings_window.set_update_check_label(lock_locale(&locale).settings().update_check.into());
    settings_window.set_update_release_label(lock_locale(&locale).settings().update_release.into());
    let labels = lock_locale(&locale).settings();
    settings_window.set_update_auto_label(labels.update_auto.into());
    settings_window.set_update_auto_description(labels.update_auto_description.into());
    settings_window.set_automatic_update_checks_enabled(
        lock_settings(&config).automatic_update_checks_enabled,
    );
    settings_window.set_update_check_available(update_distribution.can_offer_github_update());
    settings_window.set_update_store_button_label(labels.update_store.into());
    if update_distribution == Distribution::Unknown {
        settings_window.set_update_store_button_label(labels.update_unknown.into());
    }
    if update_distribution == Distribution::MicrosoftStore
        || update_distribution == Distribution::Unknown
    {
        settings_window.set_update_check_label(labels.update_store.into());
    }
    settings_window.on_open_update_release({
        let state = update_state.clone();
        move || {
            if let Some(url) = state
                .lock()
                .ok()
                .and_then(|state| state.release_url.clone())
            {
                open_url(&url);
            }
        }
    });
    settings_window.on_toggle_automatic_update_checks({
        let context = callback_context(
            &config,
            &settings_path,
            &locale,
            &settings_sender,
            &window,
            &settings_window,
            &current,
        );
        move || {
            save_setting_change(&context, |settings| {
                settings.automatic_update_checks_enabled =
                    !settings.automatic_update_checks_enabled;
            })
        }
    });
    settings_window.on_check_for_updates({
        let panel = settings_window.as_weak();
        let state = update_state.clone();
        let in_flight = update_checking.clone();
        let distribution = update_distribution;
        let locale = locale.clone();
        let manual_config = config.clone();
        let manual_settings_path = settings_path.clone();
        let manual_settings_sender = settings_sender.clone();
        move || {
            if distribution.can_offer_github_update() {
                request_update_check(
                    panel.clone(),
                    state.clone(),
                    in_flight.clone(),
                    distribution,
                    locale.clone(),
                    Some((
                        manual_config.clone(),
                        manual_settings_path.clone(),
                        manual_settings_sender.clone(),
                    )),
                );
            }
        }
    });
    let update_timer = slint::Timer::default();
    if update_distribution.can_offer_github_update() && !demo_mode {
        let panel = settings_window.as_weak();
        let state = update_state.clone();
        let in_flight = update_checking.clone();
        let update_config = config.clone();
        let update_settings_path = settings_path.clone();
        let settings_sender = settings_sender.clone();
        let locale = locale.clone();
        let startup_panel = settings_window.as_weak();
        let startup_state = update_state.clone();
        let startup_in_flight = update_checking.clone();
        let startup_config = config.clone();
        let startup_settings_path = settings_path.clone();
        let startup_settings_sender = settings_sender.clone();
        let startup_locale = locale.clone();
        update_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(60),
            move || {
                let due = {
                    let settings = lock_settings(&update_config);
                    settings.automatic_update_checks_enabled
                        && update_checker::automatic_check_due(
                            now_unix(),
                            settings.last_update_check_success_unix,
                            settings.last_automatic_update_check_attempt_unix,
                        )
                };
                if due {
                    record_automatic_check_attempt(
                        &update_config,
                        &update_settings_path,
                        &settings_sender,
                    );
                    request_update_check(
                        panel.clone(),
                        state.clone(),
                        in_flight.clone(),
                        update_distribution,
                        locale.clone(),
                        Some((
                            update_config.clone(),
                            update_settings_path.clone(),
                            settings_sender.clone(),
                        )),
                    );
                }
            },
        );
        slint::Timer::single_shot(Duration::from_secs(3), {
            move || {
                let due = {
                    let settings = lock_settings(&startup_config);
                    settings.automatic_update_checks_enabled
                        && update_checker::automatic_check_due(
                            now_unix(),
                            settings.last_update_check_success_unix,
                            settings.last_automatic_update_check_attempt_unix,
                        )
                };
                if due {
                    record_automatic_check_attempt(
                        &startup_config,
                        &startup_settings_path,
                        &startup_settings_sender,
                    );
                    request_update_check(
                        startup_panel,
                        startup_state,
                        startup_in_flight,
                        update_distribution,
                        startup_locale,
                        Some((
                            startup_config,
                            startup_settings_path,
                            startup_settings_sender,
                        )),
                    );
                }
            }
        });
    }

    #[cfg(windows)]
    let (native_tray, tray_events) = match tray::NativeTray::start(lock_locale(&locale)) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("Windows tray initialization failed: {error}. The application will exit.");
            return Ok(());
        }
    };
    #[cfg(windows)]
    settings_window.on_show_test_notification({
        let notifier = native_tray.notifier();
        let locale = locale.clone();
        let config = config.clone();
        move || {
            let labels = lock_locale(&locale).settings();
            let duration = lock_settings(&config).popup_duration_seconds;
            notifier.notify(labels.test_title, labels.test_message, duration);
        }
    });
    #[cfg(windows)]
    let worker_notifier = Some(native_tray.notifier());
    #[cfg(not(windows))]
    let worker_notifier = None;

    // One flyout window with two presentations; its state lives on the UI thread.
    #[cfg(windows)]
    let flyout = {
        let notifier = native_tray.notifier();
        flyout::Flyout::new(&window, move || notifier.icon_rect())
    };
    #[cfg(not(windows))]
    let flyout = flyout::Flyout::new(&window, || None);
    window.on_close_flyout({
        let flyout = flyout.clone();
        move || flyout.close()
    });
    window.on_start_drag({
        let flyout = flyout.clone();
        move || flyout.start_drag()
    });
    window.on_refresh_finished({
        let flyout = flyout.clone();
        move || flyout.refresh_finished()
    });
    window.window().on_close_requested({
        let flyout = flyout.clone();
        move || {
            flyout.closed_externally();
            slint::CloseRequestResponse::HideWindow
        }
    });
    {
        let flyout = flyout.clone();
        slint::Timer::single_shot(Duration::from_millis(0), move || flyout.warm_up());
    }

    // One custom tray menu for the whole run (re-used on every right-click).
    #[cfg(windows)]
    let tray_menu = TrayMenu::new()?;
    // Single dispatcher for tray events and context-menu commands, so the
    // menu runs exactly the same actions as the tray (and double-click).
    // One timer for resume refreshes: Windows sends one or two resume
    // notifications per wake-up; restarting the same timer coalesces them.
    #[cfg(windows)]
    let resume_timer = Rc::new(slint::Timer::default());
    #[cfg(windows)]
    let dispatch: Rc<dyn Fn(tray::TrayEvent)> = {
        let resume_timer = resume_timer.clone();
        let settings = settings_window.as_weak();
        let live_window = window.as_weak();
        let live_snapshot = current.clone();
        let live_config = config.clone();
        let live_locale = locale.clone();
        let flyout = flyout.clone();
        let menu = tray_menu.as_weak();
        Rc::new(move |event| match event {
            tray::TrayEvent::Hover => {
                // A visible context menu owns the interaction; no preview
                // underneath it.
                if menu
                    .upgrade()
                    .is_some_and(|menu| menu.window().is_visible())
                {
                    return;
                }
                apply_latest_to_window(&live_window, &live_snapshot, &live_config, &live_locale);
                flyout.hover();
            }
            tray::TrayEvent::Pointer => flyout.pointer_over_icon(),
            tray::TrayEvent::OpenFull => {
                apply_latest_to_window(&live_window, &live_snapshot, &live_config, &live_locale);
                flyout.open_full();
            }
            tray::TrayEvent::Refresh => {
                apply_latest_to_window(&live_window, &live_snapshot, &live_config, &live_locale);
                flyout.menu_refresh(|| request_refresh(RefreshOrigin::Manual));
            }
            tray::TrayEvent::OpenSettings => {
                if let Some(settings) = settings.upgrade() {
                    show_settings_centered(&settings);
                }
            }
            tray::TrayEvent::Quit => {
                if let Some(menu) = menu.upgrade() {
                    tray_menu::close(&menu);
                }
                let _ = slint::quit_event_loop();
            }
            tray::TrayEvent::ContextMenu { x, y } => {
                let Some(menu) = menu.upgrade() else {
                    return;
                };
                // The menu replaces a temporary preview instead of overlapping it.
                flyout.dismiss_preview();
                let settings = lock_settings(&live_config);
                let theme = adapter::theme_kind(settings.theme_mode, system_light(&settings));
                let locale = lock_locale(&live_locale);
                tray_menu::open(&menu, (x, y), theme, locale);
            }
            tray::TrayEvent::Resumed => {
                // Background refresh shortly after wake-up (network first),
                // without showing the flyout. The worker queues are bounded,
                // so this also coalesces with any pending manual refresh.
                let request_refresh = request_refresh.clone();
                resume_timer.start(
                    slint::TimerMode::SingleShot,
                    RESUME_REFRESH_DELAY,
                    move || {
                        eprintln!("RESUME: refreshing quotas after sleep/resume");
                        let _ = request_refresh(RefreshOrigin::Resume);
                    },
                );
            }
        })
    };
    #[cfg(windows)]
    {
        let menu = tray_menu.as_weak();
        let dispatch = dispatch.clone();
        tray_menu.on_activated(move |command| {
            // Close first, then run the action.
            if let Some(menu) = menu.upgrade() {
                tray_menu::close(&menu);
            }
            let event = match command {
                tray_menu::OPEN => tray::TrayEvent::OpenFull,
                tray_menu::REFRESH => tray::TrayEvent::Refresh,
                tray_menu::SETTINGS => tray::TrayEvent::OpenSettings,
                tray_menu::EXIT => tray::TrayEvent::Quit,
                _ => return,
            };
            dispatch(event);
        });
        let menu = tray_menu.as_weak();
        tray_menu.on_dismissed(move || {
            if let Some(menu) = menu.upgrade() {
                tray_menu::close(&menu);
            }
        });
    }

    #[cfg(windows)]
    let _tray_timer = {
        let tray_timer = slint::Timer::default();
        let events = tray_events;
        let dispatch = dispatch.clone();
        tray_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(50),
            move || {
                while let Ok(event) = events.try_recv() {
                    dispatch(event);
                }
            },
        );
        tray_timer
    };
    #[cfg(not(windows))]
    let _ = request_refresh;

    let cancellation = CancellationToken::new();
    let worker = if use_live {
        let provider =
            CodexRateLimitProvider::new(CodexAppServerClient::new(CodexClientConfig::default()));
        let worker_snapshot = current.clone();
        let worker_config = config.clone();
        let worker_locale = locale.clone();
        let worker_window = window.as_weak();
        let worker_settings_window = settings_window.as_weak();
        let worker_token = cancellation.clone();
        Some(thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Tokio runtime creation failed");
            runtime.block_on(run_live_workers(
                provider,
                WorkerUiContext {
                    window: worker_window,
                    settings_window: worker_settings_window,
                    current: worker_snapshot,
                    config: worker_config,
                    locale: worker_locale,
                    alerts: alerts.clone(),
                    notifier: worker_notifier,
                },
                worker_token,
                claude_bridge_enabled,
                settings_receiver,
                refresh_receiver,
                claude_refresh_receiver,
            ));
        }))
    } else {
        None
    };

    let countdown_window = window.as_weak();
    let countdown_snapshot = current.clone();
    let countdown_config = config.clone();
    let countdown_locale = locale.clone();
    let countdown_timer = slint::Timer::default();
    countdown_timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(60),
        move || {
            if let (Some(window), Ok(snapshot)) =
                (countdown_window.upgrade(), countdown_snapshot.lock())
            {
                apply_current(&window, &snapshot, &countdown_config, &countdown_locale);
            }
        },
    );

    let tray_display_notifier = native_tray.notifier();
    let tray_display_snapshot = current.clone();
    let tray_display_config = config.clone();
    let tray_icon_timer = slint::Timer::default();
    tray_icon_timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(1),
        move || {
            if let Ok(snapshot) = tray_display_snapshot.lock() {
                let settings = lock_settings(&tray_display_config);
                tray_display_notifier.update_icon(
                    snapshot.tray_values(
                        settings.codex_enabled,
                        settings.claude_enabled,
                        now_unix(),
                    ),
                    !system_light(&settings),
                    settings.warning_threshold,
                    settings.critical_threshold,
                );
            }
        },
    );

    let theme_window = window.as_weak();
    let theme_panel = settings_window.as_weak();
    let theme_timer = slint::Timer::default();
    theme_timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(2),
        move || {
            let accent = theme::system_accent_color();
            if let Some(window) = theme_window.upgrade() {
                window.set_accent_color(accent);
            }
            if let Some(panel) = theme_panel.upgrade() {
                panel.set_accent_color(accent);
            }
        },
    );

    #[cfg(windows)]
    let _popup_style_timer = {
        let popup = window.as_weak();
        let flyout = flyout.clone();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(50),
            move || {
                let Some(popup) = popup.upgrade() else {
                    return;
                };
                let _ = apply_popup_taskbar_style(&popup);
                flyout.tick();
            },
        );
        timer
    };

    // Debug-only visual review aid: open windows without tray interaction.
    #[cfg(debug_assertions)]
    {
        let preview_full = args.iter().any(|arg| arg == "--preview-flyout");
        let preview_compact = args.iter().any(|arg| arg == "--preview-compact");
        if preview_full || preview_compact {
            let flyout = flyout.clone();
            slint::Timer::single_shot(Duration::from_millis(300), move || {
                if preview_full {
                    flyout.open_full();
                } else {
                    flyout.preview_compact();
                }
            });
        }
        if let Some(tab) = args
            .iter()
            .find_map(|arg| arg.strip_prefix("--preview-settings="))
            .and_then(|tab| tab.parse::<i32>().ok())
            .filter(|tab| valid_settings_tab(*tab))
        {
            settings_window.set_selected_tab(tab);
            show_settings_centered(&settings_window);
        }
        // Font audit: render with another family (in memory only).
        if let Some(family) = args
            .iter()
            .find_map(|arg| arg.strip_prefix("--preview-font="))
        {
            let family = slint::SharedString::from(family);
            window.global::<Typography>().set_family(family.clone());
            settings_window
                .global::<Typography>()
                .set_family(family.clone());
            #[cfg(windows)]
            tray_menu.global::<Typography>().set_family(family);
        }
        // Context menu at the current cursor position, without a tray click.
        #[cfg(windows)]
        if args.iter().any(|arg| arg == "--preview-menu") {
            let dispatch = dispatch.clone();
            slint::Timer::single_shot(Duration::from_millis(600), move || {
                let (x, y) = cursor_position().unwrap_or((800, 600));
                dispatch(tray::TrayEvent::ContextMenu { x, y });
            });
        }
    }

    // The flyout starts hidden: run the loop without showing any window
    // (ComponentHandle::run() would show the flyout).
    let result = slint::run_event_loop_until_quit();
    cancellation.cancel();
    if let Some(worker) = worker {
        let _ = worker.join();
    }
    result
}

/// Threshold legend in the same left-to-right order as the scale:
/// [critical (<= critical), warning (critical..=warning), normal (> warning)],
/// built from the configured thresholds.
fn threshold_legend(
    labels: &localization::SettingsStrings,
    warning_threshold: u8,
    critical_threshold: u8,
) -> [String; 3] {
    [
        format!("{} {}%", labels.legend_critical, critical_threshold),
        // U+2060 WORD JOINER around the dash: the range "10–25%" never breaks
        // across lines when a narrow legend wraps (only between words).
        format!(
            "{} {}\u{2060}–\u{2060}{}%",
            labels.legend_warning, critical_threshold, warning_threshold
        ),
        format!("{} {}%", labels.legend_normal, warning_threshold),
    ]
}

/// Debug-only `--preview-startup=on|error`: Settings shows that startup-row
/// state regardless of the registry (visual review only).
#[cfg(debug_assertions)]
static PREVIEW_STARTUP: std::sync::OnceLock<String> = std::sync::OnceLock::new();

#[cfg(all(windows, debug_assertions))]
fn cursor_position() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut point = POINT { x: 0, y: 0 };
    (unsafe { GetCursorPos(&mut point) } != 0).then_some((point.x, point.y))
}

#[cfg(windows)]
struct SingleInstance(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl SingleInstance {
    fn acquire() -> Result<Option<Self>, ()> {
        use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
        use windows_sys::Win32::System::Threading::CreateMutexW;
        let name: Vec<u16> = "Local\\AgentUsageMonitor.Singleton\0"
            .encode_utf16()
            .collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(());
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                CloseHandle(handle);
            }
            return Ok(None);
        }
        Ok(Some(Self(handle)))
    }
}

#[cfg(windows)]
impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

type CallbackContext = (
    SharedSettings,
    std::path::PathBuf,
    SharedLocale,
    watch::Sender<AppSettings>,
    slint::Weak<MainWindow>,
    slint::Weak<SettingsWindow>,
    SharedSnapshot,
);

fn callback_context(
    config: &SharedSettings,
    path: &Path,
    locale: &SharedLocale,
    sender: &watch::Sender<AppSettings>,
    window: &MainWindow,
    panel: &SettingsWindow,
    current: &SharedSnapshot,
) -> CallbackContext {
    (
        config.clone(),
        path.to_path_buf(),
        locale.clone(),
        sender.clone(),
        window.as_weak(),
        panel.as_weak(),
        current.clone(),
    )
}

fn save_setting_change(context: &CallbackContext, change: impl FnOnce(&mut AppSettings)) {
    let _mutation = settings_mutation_guard();
    let (config, path, ..) = context;
    let mut updated = lock_settings(config);
    change(&mut updated);
    if settings::save(path, &updated).is_err() {
        return;
    }
    publish_settings(context, updated);
}

/// The one rule for the Claude status-line listener: it runs only while the
/// Claude provider is enabled **and** the user explicitly opted in. The opt-in
/// is `--enable-claude-bridge` for the EXE builds and, in the MSIX build
/// (which cannot pass launch arguments on Windows 10 1809), the off-by-default
/// `claude_bridge_listener_enabled` setting. Disabling Claude or the opt-in
/// stops the listener; neither ever turns the other on.
fn claude_bridge_wanted(bridge_argument: bool, settings: &AppSettings) -> bool {
    claude_bridge_wanted_for(packaged::is_packaged(), bridge_argument, settings)
}

fn claude_bridge_wanted_for(packaged: bool, bridge_argument: bool, settings: &AppSettings) -> bool {
    let opted_in = bridge_argument || (packaged && settings.claude_bridge_listener_enabled);
    settings.claude_enabled && opted_in
}

/// `--enable-claude-bridge` was passed to this process.
fn claude_bridge_argument() -> bool {
    static ARGUMENT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ARGUMENT.get_or_init(|| std::env::args().any(|argument| argument == "--enable-claude-bridge"))
}

/// Store build only: flip the persisted listener opt-in. The pipe worker
/// follows the published setting; the UI snapshot reflects it immediately.
fn toggle_claude_bridge_listener(context: &CallbackContext, bridge_argument: bool) {
    let _mutation = settings_mutation_guard();
    if !packaged::is_packaged() {
        return;
    }
    let (config, path, _, _, _, _, snapshot) = context;
    let mut updated = lock_settings(config);
    updated.claude_bridge_listener_enabled = !updated.claude_bridge_listener_enabled;
    if let Err(error) = settings::save(path, &updated) {
        eprintln!("CLAUDE_BRIDGE: listener preference could not be saved: {error}");
        return;
    }
    if let Ok(mut current) = snapshot.lock() {
        *current = current.with_claude_bridge(claude_bridge_wanted(bridge_argument, &updated));
    }
    publish_settings(context, updated);
}

/// Waits until the listener's wanted state is `want`. Returns false when the
/// app is shutting down (or the settings channel closed).
async fn wait_for_claude_bridge(
    settings: &mut watch::Receiver<AppSettings>,
    wanted: &(impl Fn(&AppSettings) -> bool + ?Sized),
    want: bool,
    cancellation: &CancellationToken,
) -> bool {
    loop {
        if cancellation.is_cancelled() {
            return false;
        }
        if wanted(&settings.borrow_and_update()) == want {
            return true;
        }
        tokio::select! {
            _ = cancellation.cancelled() => return false,
            changed = settings.changed() => {
                if changed.is_err() {
                    cancellation.cancelled().await;
                    return false;
                }
            }
        }
    }
}

/// Runs the status-line listener exactly while `wanted(settings)` holds.
///
/// * At most one listener future exists at a time: a new one is created only
///   after the previous one has returned or been dropped, so its pipe handle is
///   closed before the next `first_pipe_instance` server is created.
/// * When the wanted state turns false, the listener's token is cancelled and
///   its future is dropped immediately, including mid-message: nothing more is
///   accepted or parsed.
/// * A listener error is reported once through `on_failure`; the listener is
///   retried only after the wanted state turned false and true again (no
///   busy restart loop).
/// * Application cancellation stops everything and never restarts.
async fn run_bridge_lifecycle<L, F>(
    mut settings: watch::Receiver<AppSettings>,
    wanted: impl Fn(&AppSettings) -> bool,
    cancellation: CancellationToken,
    mut listen: L,
    mut on_failure: impl FnMut(),
) where
    L: FnMut(CancellationToken) -> F,
    F: std::future::Future<Output = std::io::Result<()>>,
{
    loop {
        if !wait_for_claude_bridge(&mut settings, &wanted, true, &cancellation).await {
            return;
        }
        let listener = cancellation.child_token();
        let mut stop_settings = settings.clone();
        let failed = tokio::select! {
            result = listen(listener.clone()) => result.is_err(),
            running = wait_for_claude_bridge(&mut stop_settings, &wanted, false, &cancellation) => {
                listener.cancel();
                if !running { return; }
                false
            }
        };
        if cancellation.is_cancelled() {
            return;
        }
        if failed {
            on_failure();
            if !wait_for_claude_bridge(&mut settings, &wanted, false, &cancellation).await {
                return;
            }
        }
    }
}

/// Store build only: the explicit one-time import from the EXE build.
fn import_exe_settings(context: &CallbackContext) {
    let _mutation = settings_mutation_guard();
    // Needs the Store build's own persistent folder; never imports into memory only.
    let Some(destination) = settings::settings_directory().filter(|_| packaged::is_packaged())
    else {
        return;
    };
    let (config, path, ..) = context;
    let report = settings::import_unpackaged_settings(
        &settings::unpackaged_settings_directory(),
        &destination,
        &provider_icons::all_candidate_names(),
        provider_icons::MAX_ICON_BYTES,
    );
    match report {
        Ok(report) => {
            eprintln!(
                "SETTINGS_IMPORT: settings={:?} provider_icons={}",
                report.settings, report.provider_icons
            );
            if report.settings == settings::ImportOutcome::Imported {
                match settings::load(path) {
                    Ok(mut imported) => {
                        // Windows owns the startup task state; keep what was read.
                        imported.launch_at_startup = lock_settings(config).launch_at_startup;
                        publish_settings(context, imported);
                        return;
                    }
                    Err(error) => eprintln!("SETTINGS_IMPORT: imported file unreadable: {error}"),
                }
            }
        }
        Err(error) => eprintln!("SETTINGS_IMPORT: failed: {error}"),
    }
    refresh_settings_panel(context);
}

/// Store build: whether the import row is shown (EXE settings were valid at
/// launch and the Store build has no settings of its own yet).
static STORE_IMPORT_SOURCE_VALID: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

fn store_import_available() -> bool {
    packaged::is_packaged()
        && STORE_IMPORT_SOURCE_VALID.get().copied().unwrap_or(false)
        && settings::settings_path().is_some_and(|path| !path.exists())
}

/// Re-renders the Settings window from the current settings without
/// publishing a change to the workers.
fn refresh_settings_panel(context: &CallbackContext) {
    let (config, _, locale, _, _, panel, snapshot) = context;
    let settings = lock_settings(config);
    if let (Some(panel), Ok(snapshot)) = (panel.upgrade(), snapshot.lock()) {
        update_settings_window(
            &panel,
            &settings,
            lock_locale(locale),
            system_light(&settings),
            &snapshot,
            now_unix(),
        );
    }
}

/// Requests submitted at launch: the MSIX startup task is only read, never
/// changed. EXE builds reconcile their Run value synchronously instead.
fn launch_startup_request(state: packaged::PackageState) -> Option<autostart::StartupRequest> {
    (state == packaged::PackageState::Packaged).then_some(autostart::StartupRequest::Refresh)
}

/// UI thread: applies a finished StartupTask request.
fn apply_packaged_startup_result(
    context: &CallbackContext,
    request: autostart::StartupRequest,
    result: autostart::StartupResult,
) {
    let _mutation = settings_mutation_guard();
    let (config, path, ..) = context;
    let mut updated = lock_settings(config);
    match &result {
        Ok(status) => {
            updated.launch_at_startup = status.enabled;
            if let autostart::StartupRequest::Set(want) = request {
                if status.enabled != want {
                    eprintln!("AUTOSTART: Windows kept the startup task as {status:?}");
                }
                // Only a user's change is persisted; reads stay in memory.
                if let Err(error) = settings::save(path, &updated) {
                    eprintln!("AUTOSTART: preference could not be saved: {error}");
                }
            }
        }
        Err(error) => eprintln!("AUTOSTART: startup task {request:?} failed: {error}"),
    }
    publish_settings(context, updated);
}

fn toggle_autostart(context: &CallbackContext) {
    let _mutation = settings_mutation_guard();
    if packaged::is_packaged() {
        let controller = autostart::packaged_controller();
        let request = autostart::toggle_request(controller.last().as_ref());
        if controller.submit(request) {
            refresh_settings_panel(context);
        }
        return;
    }
    let (config, path, ..) = context;
    let mut updated = lock_settings(config);
    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("AUTOSTART: could not resolve executable: {error}");
            show_autostart_unverified(context);
            return;
        }
    };
    let actual_before = match autostart::is_enabled_for(&executable) {
        Ok(actual) => actual,
        Err(error) => {
            eprintln!("AUTOSTART: could not read current registration state: {error}");
            show_autostart_unverified(context);
            return;
        }
    };
    let requested = !actual_before;
    let claude_bridge = std::env::args().any(|argument| argument == "--enable-claude-bridge");
    if let Err(error) = autostart::set_enabled(requested, &executable, claude_bridge) {
        eprintln!("AUTOSTART: registration change failed: {error}");
        show_autostart_unverified(context);
        return;
    }
    let actual = match autostart::is_enabled_for(&executable) {
        Ok(actual) => actual,
        Err(error) => {
            eprintln!("AUTOSTART: registration changed but could not verify it: {error}");
            requested
        }
    };
    if actual != requested {
        eprintln!("AUTOSTART: requested state did not match the registered state");
    }
    updated.launch_at_startup = actual;
    if let Err(error) = settings::save(path, &updated) {
        eprintln!("AUTOSTART: registry changed but preference could not be saved: {error}");
    }
    publish_settings(context, updated);
}

fn show_autostart_unverified(context: &CallbackContext) {
    let (_, _, locale, _, _, panel, _) = context;
    if let Some(panel) = panel.upgrade() {
        let labels = lock_locale(locale).settings();
        let row = startup_row_error();
        panel.set_startup_state_known(row.interactive);
        panel.set_startup_busy(row.busy);
        panel.set_startup_enabled(row.enabled);
        panel.set_startup_description(startup_text(&labels, row.text).into());
    }
}

/// Which explanation the sign-in row shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StartupText {
    Normal,
    WindowsControlled,
    Policy,
    Error,
    Checking,
}

/// What the sign-in row shows. `interactive` maps to `startup-state-known`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StartupRow {
    enabled: bool,
    interactive: bool,
    busy: bool,
    text: StartupText,
}

/// Maps the last known state (and whether a request is in flight) to the row.
/// A locked state (Windows or policy) is shown as it is and is not operable.
fn startup_row(last: Option<&autostart::StartupResult>, busy: bool) -> StartupRow {
    use autostart::StartupControl;
    if busy || last.is_none() {
        return StartupRow {
            enabled: matches!(last, Some(Ok(status)) if status.enabled),
            interactive: false,
            busy: true,
            text: StartupText::Checking,
        };
    }
    match last {
        Some(Ok(status)) => StartupRow {
            enabled: status.enabled,
            interactive: status.control == StartupControl::App,
            busy: false,
            text: match status.control {
                StartupControl::App => StartupText::Normal,
                StartupControl::DisabledInWindows => StartupText::WindowsControlled,
                StartupControl::Policy => StartupText::Policy,
            },
        },
        _ => StartupRow {
            enabled: false,
            interactive: false,
            busy: false,
            text: StartupText::Error,
        },
    }
}

fn startup_row_error() -> StartupRow {
    startup_row(Some(&Err(String::new())), false)
}

fn startup_text(labels: &localization::SettingsStrings, text: StartupText) -> &'static str {
    match text {
        StartupText::Normal => labels.startup_description,
        StartupText::WindowsControlled => labels.startup_windows_controlled,
        StartupText::Policy => labels.startup_policy_controlled,
        StartupText::Error => labels.startup_error,
        StartupText::Checking => labels.startup_checking,
    }
}

/// Whether launch-time reconciliation may write the sign-in registration.
/// EXE builds rewrite their HKCU `Run` value so it follows a moved or updated
/// executable. The MSIX startup task survives Store updates and belongs to
/// Windows: at launch it is only read (asynchronously, see
/// `launch_startup_request`), and it is enabled solely by the user's own
/// toggle. Without confirmed package identity nothing is written either.
fn launch_reconcile_writes_registration(state: packaged::PackageState) -> bool {
    state == packaged::PackageState::Unpackaged
}

fn reconcile_autostart(config: &SharedSettings, path: &Path, claude_bridge: bool) {
    let _mutation = settings_mutation_guard();
    if packaged::is_packaged() {
        // Read later by the StartupTask controller, off the UI thread.
        return;
    }
    let Ok(executable) = std::env::current_exe() else {
        eprintln!("AUTOSTART: could not resolve executable; registration was not changed");
        return;
    };
    let mut settings = lock_settings(config);
    if !launch_reconcile_writes_registration(packaged::state()) {
        // Unknown identity: show the Run value as it is, change nothing, save nothing.
        match autostart::is_enabled_for(&executable) {
            Ok(actual) => settings.launch_at_startup = actual,
            Err(error) => eprintln!("AUTOSTART: registration state could not be read: {error}"),
        }
        if let Ok(mut current) = config.lock() {
            *current = settings;
        }
        return;
    }
    if let Err(error) =
        autostart::set_enabled(settings.launch_at_startup, &executable, claude_bridge)
    {
        eprintln!("AUTOSTART: startup registration could not be reconciled: {error}");
        if let Ok(actual) = autostart::is_enabled_for(&executable) {
            settings.launch_at_startup = actual;
        }
    } else {
        settings.launch_at_startup = match autostart::is_enabled_for(&executable) {
            Ok(actual) => actual,
            Err(error) => {
                eprintln!("AUTOSTART: registration reconciled but verification failed: {error}");
                settings.launch_at_startup
            }
        };
    }
    if let Err(error) = settings::save(path, &settings) {
        eprintln!("AUTOSTART: actual registration state could not be saved: {error}");
    }
    if let Ok(mut current) = config.lock() {
        *current = settings;
    }
}

fn publish_settings(context: &CallbackContext, updated: AppSettings) {
    let (config, _path, locale, sender, window, panel, snapshot) = context;
    if let Ok(mut current) = config.lock() {
        *current = updated.clone();
    }
    let resolved = resolve_locale(&updated);
    tray::set_locale(resolved);
    if let Ok(mut current_locale) = locale.lock() {
        *current_locale = resolved;
    }
    let _ = sender.send(updated.clone());
    // The "waiting for the status line" state follows the same rule as the
    // listener (e.g. turning Claude off stops it).
    if let Ok(mut current) = snapshot.lock() {
        *current =
            current.with_claude_bridge(claude_bridge_wanted(claude_bridge_argument(), &updated));
    }
    if let (Some(window), Ok(snapshot)) = (window.upgrade(), snapshot.lock()) {
        if let Some(panel) = panel.upgrade() {
            update_settings_window(
                &panel,
                &updated,
                resolved,
                system_light(&updated),
                &snapshot,
                now_unix(),
            );
        }
        apply_current(&window, &snapshot, config, locale);
    } else if let (Some(panel), Ok(snapshot)) = (panel.upgrade(), snapshot.lock()) {
        update_settings_window(
            &panel,
            &updated,
            resolved,
            system_light(&updated),
            &snapshot,
            now_unix(),
        );
    }
}

async fn run_live_workers<P>(
    provider: P,
    context: WorkerUiContext,
    cancellation: CancellationToken,
    claude_bridge_enabled: bool,
    settings: watch::Receiver<AppSettings>,
    refresh_requests: mpsc::Receiver<ManualRefreshRequest>,
    claude_refresh_requests: mpsc::Receiver<ManualRefreshRequest>,
) where
    P: RateLimitProvider + 'static,
{
    let (sender, mut receiver) = mpsc::channel::<ClaudeStatusLineSnapshot>(4);
    let (cli_sender, mut cli_receiver) = mpsc::channel::<claude_refresher::ClaudeRefreshEvent>(2);
    let claude_window = context.window.clone();
    let claude_snapshot = context.current.clone();
    let claude_config = context.config.clone();
    let claude_locale = context.locale.clone();
    let claude_cancellation = cancellation.clone();
    let claude_settings = settings.clone();
    let cli_settings = settings.clone();
    let pipe_settings = settings.clone();
    let pipe_wait_cancellation = cancellation.clone();
    let claude_alerts = context.alerts.clone();
    let claude_settings_window = context.settings_window.clone();
    let claude_notifier = context.notifier.clone();
    let claude_updates = async move {
        let mut state = ProviderState::new("claude");
        // Always drained, so a listener started later never blocks on a full
        // channel; a message that arrives after the listener became unwanted
        // is dropped.
        let mut bridge_open = true;
        loop {
            tokio::select! {
                _ = claude_cancellation.cancelled() => break,
                snapshot = receiver.recv(), if bridge_open => {
                    let Some(snapshot) = snapshot else { bridge_open = false; continue };
                    if !claude_bridge_wanted(claude_bridge_enabled, &claude_settings.borrow()) { continue; }
                    let provider = snapshot.to_provider_snapshot();
                    state.record_success(provider.limits, None, snapshot.snapshot_at);
                    let updated = if let Ok(mut current) = claude_snapshot.lock() {
                        *current = current.replace_claude(state.snapshot().clone(), snapshot);
                        Some(current.clone())
                    } else { None };
                    if let Some(updated) = updated {
                        if let Ok(mut alert_runtime) = claude_alerts.lock() {
                            let events = alert_runtime.refresh(&updated, &lock_settings(&claude_config), now_unix());
                            deliver_alerts(&claude_notifier, events, lock_locale(&claude_locale));
                        }
                        queue_ui_snapshot(claude_window.clone(), claude_settings_window.clone(), claude_snapshot.clone(), claude_config.clone(), claude_locale.clone());
                    }
                }
                usage = cli_receiver.recv() => {
                    let Some(usage) = usage else { continue };
                    if !claude_settings.borrow().claude_enabled
                        || !claude_settings.borrow().claude_cli_refresh_enabled
                    { continue; }
                    let (request_id, updated) = match usage {
                        claude_refresher::ClaudeRefreshEvent::Completed { request_id, snapshot } => {
                            let cache_started = std::time::Instant::now();
                            let updated = if let Ok(mut current) = claude_snapshot.lock() {
                                *current = current.merge_claude_usage(snapshot);
                                Some(current.clone())
                            } else { None };
                            eprintln!("CLAUDE_REFRESH: cache_updated request_id={} cache_ms={}", request_id.map_or_else(|| "background".into(), |id| id.to_string()), cache_started.elapsed().as_millis());
                            (request_id, updated)
                        }
                        claude_refresher::ClaudeRefreshEvent::Failed { request_id, timed_out, cli_missing } => {
                            let updated = if let Ok(mut current) = claude_snapshot.lock() {
                                *current = if cli_missing {
                                    current.replace_claude_cli_missing()
                                } else {
                                    current.replace_claude_error()
                                };
                                Some(current.clone())
                            } else { None };
                            let kind = if timed_out { "timed_out" } else { "failed" };
                            eprintln!("CLAUDE_REFRESH: cache_updated request_id={} result={kind} retained_last_good=true", request_id.map_or_else(|| "background".into(), |id| id.to_string()));
                            (request_id, updated)
                        }
                    };
                    if let Some(updated) = updated {
                        if let Ok(mut alert_runtime) = claude_alerts.lock() {
                            let events = alert_runtime.refresh(&updated, &lock_settings(&claude_config), now_unix());
                            deliver_alerts(&claude_notifier, events, lock_locale(&claude_locale));
                        }
                        queue_ui_snapshot_for_refresh(
                            claude_window.clone(),
                            claude_settings_window.clone(),
                            claude_snapshot.clone(),
                            claude_config.clone(),
                            claude_locale.clone(),
                            request_id,
                        );
                    }
                }
            }
        }
    };
    let pipe_snapshot = context.current.clone();
    let pipe_window = context.window.clone();
    let pipe_settings_window = context.settings_window.clone();
    let pipe_config = context.config.clone();
    let pipe_locale = context.locale.clone();
    // The listener runs exactly while `claude_bridge_wanted` holds (Claude on
    // and explicitly opted in); see `run_bridge_lifecycle`.
    let pipe_updates = run_bridge_lifecycle(
        pipe_settings,
        move |settings: &AppSettings| claude_bridge_wanted(claude_bridge_enabled, settings),
        pipe_wait_cancellation,
        move |listener| run_claude_status_pipe(sender.clone(), listener),
        move || {
            if let Ok(mut current) = pipe_snapshot.lock() {
                *current = current.replace_claude_error();
            }
            queue_ui_snapshot(
                pipe_window.clone(),
                pipe_settings_window.clone(),
                pipe_snapshot.clone(),
                pipe_config.clone(),
                pipe_locale.clone(),
            );
        },
    );
    let codex = refresh_loop(
        provider,
        context,
        cancellation.clone(),
        settings,
        refresh_requests,
    );
    let cli = claude_refresher::run(
        cli_settings,
        cli_sender,
        claude_refresh_requests,
        cancellation,
    );
    tokio::join!(codex, claude_updates, pipe_updates, cli);
}

async fn refresh_loop<P>(
    provider: P,
    context: WorkerUiContext,
    cancellation: CancellationToken,
    mut settings: watch::Receiver<AppSettings>,
    mut refresh_requests: mpsc::Receiver<ManualRefreshRequest>,
) where
    P: RateLimitProvider + 'static,
{
    let mut state = ProviderState::new(provider.id());
    let mut interval = tokio::time::interval(REFRESH_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut enabled = settings.borrow().codex_enabled;
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => break,
            changed = settings.changed() => {
                if changed.is_err() { break; }
                let updated_settings = settings.borrow().clone();
                if let Ok(mut alert_runtime) = context.alerts.lock() {
                    let _ = alert_runtime.engine.set_settings(alert_settings(&updated_settings));
                }
                let next = updated_settings.codex_enabled;
                if next && !enabled {
                    enabled = true;
                    refresh_codex(&provider, &mut state, &context, &cancellation, None).await;
                } else { enabled = next; }
            },
            _ = interval.tick(), if enabled => {
                refresh_codex(&provider, &mut state, &context, &cancellation, None).await;
            }
            request = refresh_requests.recv() => {
                if request.is_none() { break; }
                if enabled {
                    let request = request.unwrap();
                    if skip_redundant_resume_refresh(request.origin, state.snapshot().last_success_unix_seconds, now_unix()) {
                        eprintln!("REFRESH: skipped id={} provider=codex reason=already_refreshed_after_resume", request.id);
                    } else {
                        refresh_codex(&provider, &mut state, &context, &cancellation, Some(request.id)).await;
                    }
                    // After sleep the 5-minute tick can be overdue and ready at the
                    // same moment; restarting it here keeps it from repeating the
                    // request-driven refresh straight away.
                    interval.reset();
                } else if let Some(request) = request {
                    eprintln!("REFRESH: skipped id={} provider=codex reason=disabled_at_execution", request.id);
                }
                let settings = lock_settings(&context.config);
                if !settings.claude_enabled || !settings.claude_cli_refresh_enabled {
                    // Claude is not part of this request; Codex completes it.
                    let _ = context.window.upgrade_in_event_loop(|window| window.invoke_refresh_finished());
                }
            }
        }
    }
}

async fn refresh_codex<P: RateLimitProvider>(
    provider: &P,
    state: &mut ProviderState,
    context: &WorkerUiContext,
    cancellation: &CancellationToken,
    request_id: Option<u64>,
) {
    eprintln!(
        "REFRESH: started id={} provider=codex",
        request_id.map_or_else(|| "background".into(), |id| id.to_string())
    );
    let provider_started = std::time::Instant::now();
    let result = provider.read_rate_limits(cancellation.child_token()).await;
    eprintln!(
        "PROVIDER_LATENCY: codex response_ms={}",
        provider_started.elapsed().as_millis()
    );
    let succeeded = result.is_ok();
    match result {
        Ok(limits) => state.record_success(limits, None, now_unix()),
        Err(error) if error.code == "codex_cancelled" && cancellation.is_cancelled() => return,
        Err(error) => state.record_failure(status_for(&error), error),
    }
    eprintln!(
        "REFRESH: {} id={} provider=codex",
        if succeeded { "completed" } else { "failed" },
        request_id.map_or_else(|| "background".into(), |id| id.to_string())
    );
    let updated = if let Ok(mut current) = context.current.lock() {
        *current = current.replace_codex(state.snapshot().clone());
        Some(current.clone())
    } else {
        None
    };
    if let Some(updated) = updated {
        if let Ok(mut alert_runtime) = context.alerts.lock() {
            let events =
                alert_runtime.refresh(&updated, &lock_settings(&context.config), now_unix());
            deliver_alerts(&context.notifier, events, lock_locale(&context.locale));
        }
        queue_ui_snapshot(
            context.window.clone(),
            context.settings_window.clone(),
            context.current.clone(),
            context.config.clone(),
            context.locale.clone(),
        );
    }
}

fn apply_current(
    window: &MainWindow,
    snapshot: &adapter::UiSnapshot,
    config: &SharedSettings,
    locale: &SharedLocale,
) {
    let settings = lock_settings(config);
    let locale = lock_locale(locale);
    adapter::apply_config(
        window,
        snapshot,
        now_unix(),
        &settings,
        locale,
        system_light(&settings),
    );
}

fn apply_latest_to_window(
    window: &slint::Weak<MainWindow>,
    snapshot: &SharedSnapshot,
    config: &SharedSettings,
    locale: &SharedLocale,
) {
    if let (Some(window), Ok(snapshot)) = (window.upgrade(), snapshot.lock()) {
        apply_current(&window, &snapshot, config, locale);
    }
}

/// Provider workers publish snapshots through Slint's event loop. The shared
/// cache remains authoritative, so a flyout opened later can also apply its
/// newest value even if it was hidden when this callback was queued.
fn queue_ui_snapshot(
    window: slint::Weak<MainWindow>,
    panel: slint::Weak<SettingsWindow>,
    snapshot: SharedSnapshot,
    config: SharedSettings,
    locale: SharedLocale,
) {
    queue_ui_snapshot_for_refresh(window, panel, snapshot, config, locale, None);
}

fn queue_ui_snapshot_for_refresh(
    window: slint::Weak<MainWindow>,
    panel: slint::Weak<SettingsWindow>,
    snapshot: SharedSnapshot,
    config: SharedSettings,
    locale: SharedLocale,
    refresh_id: Option<u64>,
) {
    let queued_at = std::time::Instant::now();
    let _ = window.upgrade_in_event_loop(move |window| {
        eprintln!(
            "UI_PROPAGATION: snapshot_to_event_loop_ms={}",
            queued_at.elapsed().as_millis()
        );
        let Ok(snapshot) = snapshot.lock() else {
            return;
        };
        let snapshot = snapshot.clone();
        apply_current(&window, &snapshot, &config, &locale);
        if let Some(id) = refresh_id {
            eprintln!(
                "CLAUDE_REFRESH: ui_updated request_id={id} propagation_ms={}",
                queued_at.elapsed().as_millis()
            );
            window.invoke_refresh_finished();
        }
        if let Some(panel) = panel.upgrade() {
            let settings = lock_settings(&config);
            update_settings_window(
                &panel,
                &settings,
                lock_locale(&locale),
                system_light(&settings),
                &snapshot,
                now_unix(),
            );
        }
    });
}

fn update_settings_window(
    window: &SettingsWindow,
    settings: &AppSettings,
    locale: Locale,
    light: bool,
    snapshot: &adapter::UiSnapshot,
    now: i64,
) {
    let labels = locale.settings();
    use slint::SharedString;
    window.set_codex_enabled(settings.codex_enabled);
    window.set_claude_enabled(settings.claude_enabled);
    window.set_claude_cli_refresh_enabled(settings.claude_cli_refresh_enabled);
    window.set_store_build(packaged::is_packaged());
    window.set_claude_bridge_listener_enabled(settings.claude_bridge_listener_enabled);
    window.set_store_import_available(store_import_available());
    // Visible, not silent: without persistent storage changes cannot be saved.
    window.set_storage_warning(
        if settings::storage_available() {
            ""
        } else {
            labels.storage_unavailable
        }
        .into(),
    );
    // EXE: one synchronous HKCU `Run` read. MSIX: the StartupTask controller's
    // last result, never a WinRT call on this (UI) thread; a locked task
    // (Windows or policy) is shown with an explanation and is not operable.
    let startup_row = if packaged::is_packaged() {
        let controller = autostart::packaged_controller();
        startup_row(controller.last().as_ref(), controller.busy())
    } else {
        let status = std::env::current_exe()
            .map_err(|error| error.to_string())
            .and_then(|executable| autostart::status_for(&executable))
            .inspect_err(|error| {
                eprintln!("AUTOSTART: could not read actual registration state: {error}");
            });
        startup_row(Some(&status), false)
    };
    // Debug-only visual review of the ON / unverified states (UI values only;
    // the registry is neither read differently nor changed).
    #[cfg(debug_assertions)]
    let startup_row = match PREVIEW_STARTUP.get().map(String::as_str) {
        Some("on") => StartupRow {
            enabled: true,
            interactive: true,
            busy: false,
            text: StartupText::Normal,
        },
        Some("error") => startup_row_error(),
        _ => startup_row,
    };
    window.set_startup_state_known(startup_row.interactive);
    window.set_startup_busy(startup_row.busy);
    window.set_startup_enabled(startup_row.enabled);
    let icons = provider_icons::local();
    let scale = window.window().scale_factor();
    let codex_icon = icons.icon(provider_icons::Provider::Codex, light, 16.0, scale);
    let claude_icon = icons.icon(provider_icons::Provider::Claude, light, 16.0, scale);
    window.set_codex_icon_available(codex_icon.is_some());
    window.set_codex_icon(codex_icon.unwrap_or_default());
    window.set_claude_icon_available(claude_icon.is_some());
    window.set_claude_icon(claude_icon.unwrap_or_default());
    window.set_warning_threshold(i32::from(settings.warning_threshold));
    window.set_critical_threshold(i32::from(settings.critical_threshold));
    window.set_alerts_enabled(settings.alerts_enabled);
    window.set_exhausted_alerts_enabled(settings.exhausted_alerts_enabled);
    window.set_include_weekly(settings.include_weekly);
    window.set_theme_kind(adapter::theme_kind(settings.theme_mode, light));
    window.invoke_apply_theme_palette();
    theme::apply_title_bar(window.window(), settings.theme_mode);
    window.set_theme_options(
        [
            SharedString::from(labels.graphite),
            SharedString::from(labels.navy),
            SharedString::from(labels.light),
        ]
        .into(),
    );
    window.set_theme_index(match settings.theme_mode {
        ThemeMode::Graphite => 0,
        ThemeMode::BluishDark => 1,
        ThemeMode::Light => 2,
    });
    window.set_language_options(
        [
            "Українська".into(),
            "English".into(),
            "Polski".into(),
            "Русский".into(),
        ]
        .into(),
    );
    window.set_language_index(match settings.language {
        Language::Ukrainian => 0,
        Language::English => 1,
        Language::Polish => 2,
        Language::Russian => 3,
    });
    let warning_options: Vec<SharedString> = [25, 30, 40, 50, 75, 100]
        .into_iter()
        .map(|value| format!("{value}%").into())
        .collect();
    window.set_warning_options(warning_options.as_slice().into());
    window.set_warning_index(
        [25, 30, 40, 50, 75, 100]
            .iter()
            .position(|value| *value == settings.warning_threshold)
            .unwrap_or(0) as i32,
    );
    let critical_values: Vec<u8> = [5, 10, 15, 20]
        .into_iter()
        .filter(|value| *value <= settings.warning_threshold)
        .collect();
    let critical_options: Vec<SharedString> = critical_values
        .iter()
        .map(|value| format!("{value}%").into())
        .collect();
    window.set_critical_options(critical_options.as_slice().into());
    window.set_critical_index(
        critical_values
            .iter()
            .position(|value| *value == settings.critical_threshold)
            .unwrap_or(0) as i32,
    );
    let durations = [4, 8, 12, 20, 30];
    let duration_options: Vec<SharedString> = durations
        .into_iter()
        .map(|value| format!("{value} {}", labels.popup_seconds).into())
        .collect();
    window.set_duration_options(duration_options.as_slice().into());
    window.set_duration_index(
        durations
            .iter()
            .position(|value| *value == settings.popup_duration_seconds)
            .unwrap_or(1) as i32,
    );

    let (codex_state, codex_description) =
        snapshot.settings_status("codex", settings.codex_enabled, now, locale);
    window.set_codex_state(codex_state.into());
    window.set_codex_description(codex_description.into());
    window.set_codex_state_level(snapshot.settings_status_level(
        "codex",
        settings.codex_enabled,
        now,
    ));
    let (claude_state, claude_description) =
        snapshot.settings_status("claude", settings.claude_enabled, now, locale);
    window.set_claude_state(claude_state.into());
    window.set_claude_description(claude_description.into());
    window.set_claude_state_level(snapshot.settings_status_level(
        "claude",
        settings.claude_enabled,
        now,
    ));

    let section = |value: &str| SharedString::from(value.to_uppercase());
    window.set_title_label(labels.title.into());
    window.set_general_label(labels.general.into());
    window.set_notifications_label(labels.notifications.into());
    window.set_about_label(labels.about.into());
    window.set_appearance_label(section(labels.appearance));
    window.set_providers_label(section(labels.providers));
    window.set_thresholds_label(section(labels.thresholds));
    window.set_display_label(section(labels.display));
    window.set_connections_label(section(labels.connections));
    window.set_theme_name_label(labels.theme.into());
    window.set_language_name_label(labels.language.into());
    window.set_alerts_title(labels.alerts.into());
    window.set_alerts_description(labels.alerts_description.into());
    window.set_warning_name_label(labels.warning.into());
    window.set_warning_description(labels.warning_description.into());
    window.set_critical_name_label(labels.critical.into());
    window.set_critical_description(labels.critical_description.into());
    window.set_exhausted_name_label(labels.exhausted.into());
    window.set_exhausted_description(labels.exhausted_description.into());
    window.set_weekly_name_label(labels.weekly.into());
    window.set_weekly_description(labels.weekly_description.into());
    window.set_duration_name_label(labels.duration.into());
    window.set_duration_description(labels.duration_description.into());
    let [critical, warning, normal] = threshold_legend(
        &labels,
        settings.warning_threshold,
        settings.critical_threshold,
    );
    window.set_legend_normal(normal.into());
    window.set_legend_warning(warning.into());
    window.set_legend_critical(critical.into());
    window.set_show_test_label(labels.show_test.into());
    window.set_source_note(labels.claude_note.into());
    window.set_claude_refresh_label(labels.claude_refresh.into());
    window.set_claude_refresh_description(labels.claude_refresh_description.into());
    window.set_claude_bridge_label(labels.claude_bridge.into());
    window.set_claude_bridge_description(labels.claude_bridge_description.into());
    if let Ok(mut context) = STATUSLINE_CONTEXT.lock() {
        *context = Some((
            locale,
            claude_bridge_wanted(claude_bridge_argument(), settings),
        ));
    }
    apply_statusline_row(window);
    window.set_startup_label(labels.startup.into());
    window.set_startup_description(startup_text(&labels, startup_row.text).into());
    window.set_import_label(labels.import_title.into());
    window.set_import_description(labels.import_description.into());
    window.set_import_button_label(labels.import_button.into());

    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    window.set_app_version(version.as_str().into());
    window.set_about_product("AgentUsageMonitor".into());
    window.set_about_tagline(labels.about_tagline.into());
    let distribution = Distribution::current();
    window.set_update_check_label(if distribution == Distribution::MicrosoftStore {
        labels.update_store.into()
    } else {
        labels.update_check.into()
    });
    let update_status = lock_update_status();
    window.set_update_checking(matches!(update_status, UpdateStatus::Checking));
    set_update_status_text(window, labels, &update_status);
    window.set_update_auto_label(labels.update_auto.into());
    window.set_update_auto_description(labels.update_auto_description.into());
    window.set_automatic_update_checks_enabled(settings.automatic_update_checks_enabled);
    window.set_update_release_label(labels.update_release.into());
    window.set_update_distribution_label(
        update_checker::distribution_label(distribution, locale).into(),
    );
    window.set_update_check_available(distribution.can_offer_github_update());
    window.set_update_store_button_label(if distribution == Distribution::MicrosoftStore {
        labels.update_store.into()
    } else {
        labels.update_unknown.into()
    });
    if distribution == Distribution::MicrosoftStore {
        window.set_update_check_label(labels.update_store.into());
    } else {
        window.set_update_check_label(labels.update_check.into());
    }
    window.set_about_version(format!("{}  {}", labels.version, env!("CARGO_PKG_VERSION")).into());
    window.set_about_author(format!("{}  Valentyn Fetisov", labels.author).into());
    window.set_about_repository(labels.repository.into());
    window.set_about_report_issue(labels.report_issue.into());
    window.set_about_license(labels.license.into());
    window.set_about_technologies(labels.technologies.into());
    window.set_about_feature_tray(labels.feature_tray.into());
    window.set_about_feature_local(labels.feature_local.into());
    window.set_about_feature_agents(labels.feature_agents.into());
    window.set_codex_source(labels.codex_source.into());
    window.set_claude_source(labels.claude_source.into());
    window.set_legal_note(labels.legal.into());
    window.set_copyright_line(format!("© 2026 Valentyn Fetisov · {}", labels.license).into());
}

fn request_update_check(
    panel: slint::Weak<SettingsWindow>,
    state: Arc<Mutex<UiState>>,
    in_flight: Arc<AtomicBool>,
    distribution: Distribution,
    locale: SharedLocale,
    persistence: Option<(SharedSettings, PathBuf, watch::Sender<AppSettings>)>,
) {
    if !distribution.can_offer_github_update() || in_flight.swap(true, Ordering::AcqRel) {
        return;
    }
    if let Ok(mut state) = state.lock() {
        state.update_available = false;
    }
    set_update_status(UpdateStatus::Checking);
    if let Some(panel) = panel.upgrade() {
        panel.set_update_checking(true);
        panel.set_update_available(false);
        panel.set_update_release_visible(false);
        panel.set_update_status(lock_locale(&locale).settings().update_checking.into());
    }
    thread::spawn(move || {
        let result = update_checker::check_latest();
        in_flight.store(false, Ordering::Release);
        let update_status = match &result {
            Ok(info) if info.update_available => UpdateStatus::Available {
                current: info.current_version.clone(),
                latest: info.latest_version.clone(),
            },
            Ok(_) => UpdateStatus::Current,
            Err(update_checker::CheckError::Offline) => UpdateStatus::Offline,
            Err(update_checker::CheckError::Http(code)) => UpdateStatus::Http(*code),
            Err(update_checker::CheckError::InvalidMetadata) => UpdateStatus::Invalid,
            Err(update_checker::CheckError::Timeout) => UpdateStatus::Timeout,
        };
        let url = result
            .as_ref()
            .ok()
            .filter(|info| info.update_available)
            .map(|info| info.release_url.clone());
        let available = result.as_ref().is_ok_and(|info| info.update_available);
        if let Ok(mut model) = state.lock() {
            model.release_url = url;
            model.update_available = available;
        }
        set_update_status(update_status.clone());
        if result.is_ok() {
            if let Some((config, path, sender)) = persistence {
                let _mutation = settings_mutation_guard();
                let mut settings = lock_settings(&config);
                settings.last_update_check_success_unix = Some(now_unix());
                let save_result = settings::save(&path, &settings);
                if let Ok(mut current) = config.lock() {
                    *current = settings.clone();
                }
                let _ = sender.send(settings);
                if let Err(error) = save_result {
                    eprintln!("Could not persist successful update check timestamp: {error}");
                }
            }
        }
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(panel) = panel.upgrade() {
                let labels = lock_locale(&locale).settings();
                panel.set_update_release_label(labels.update_release.into());
                panel.set_update_distribution_label(
                    update_checker::distribution_label(distribution, lock_locale(&locale)).into(),
                );
                panel.set_update_check_label(if distribution == Distribution::MicrosoftStore {
                    labels.update_store.into()
                } else {
                    labels.update_check.into()
                });
                panel.set_update_checking(false);
                panel.set_update_available(available && distribution.can_offer_github_update());
                panel.set_update_release_visible(
                    available && distribution.can_offer_github_update(),
                );
                set_update_status_text(&panel, labels, &update_status);
            }
        });
    });
}

fn lock_update_status() -> UpdateStatus {
    UPDATE_STATUS
        .lock()
        .map(|status| status.clone())
        .unwrap_or_default()
}

fn set_update_status(status: UpdateStatus) {
    if let Ok(mut current) = UPDATE_STATUS.lock() {
        *current = status;
    }
}

fn record_automatic_check_attempt(
    config: &SharedSettings,
    path: &Path,
    sender: &watch::Sender<AppSettings>,
) {
    let _mutation = settings_mutation_guard();
    let mut settings = lock_settings(config);
    settings.last_automatic_update_check_attempt_unix = Some(now_unix());
    let save_result = settings::save(path, &settings);
    if let Ok(mut current) = config.lock() {
        *current = settings.clone();
    }
    let _ = sender.send(settings);
    if let Err(error) = save_result {
        eprintln!("Could not persist automatic update check attempt: {error}");
    }
}

fn settings_mutation_guard() -> std::sync::MutexGuard<'static, ()> {
    SETTINGS_MUTATION_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn set_update_status_text(
    window: &SettingsWindow,
    labels: localization::SettingsStrings,
    status: &UpdateStatus,
) {
    use slint::SharedString;
    let text: SharedString = match status {
        UpdateStatus::Idle => "".into(),
        UpdateStatus::Checking => labels.update_checking.into(),
        UpdateStatus::Available { current, latest } => {
            format!("{}: v{} → v{}", labels.update_available, current, latest).into()
        }
        UpdateStatus::Current => labels.update_current.into(),
        UpdateStatus::Offline => labels.update_offline.into(),
        UpdateStatus::Http(code) => format!("{} ({code})", labels.update_http).into(),
        UpdateStatus::Invalid => labels.update_invalid.into(),
        UpdateStatus::Timeout => labels.update_timeout.into(),
    };
    window.set_update_status(text);
}

/// Last read Claude Code statusLine state; read when Settings opens, never
/// on every UI refresh. `None` until the first read.
static STATUSLINE_STATE: std::sync::Mutex<Option<claude_statusline::StatusLineState>> =
    std::sync::Mutex::new(None);
/// Locale and bridge opt-in used to render the statusLine row (kept from the
/// last full Settings update, so `show_settings_centered` can re-render it).
static STATUSLINE_CONTEXT: std::sync::Mutex<Option<(Locale, bool)>> = std::sync::Mutex::new(None);

/// Re-reads the Claude Code statusLine setting (read-only, bounded).
fn refresh_statusline_state() {
    let state = claude_statusline::user_settings_path()
        .map(|path| claude_statusline::inspect(&path))
        .unwrap_or(claude_statusline::StatusLineState::SettingsNotFound);
    if let Ok(mut current) = STATUSLINE_STATE.lock() {
        *current = Some(state);
    }
}

/// The command this AUM copy would suggest (None when its path is refused).
fn bridge_command() -> Result<String, claude_statusline::CommandError> {
    let directory = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    claude_statusline::suggested_command(packaged::is_packaged(), directory.as_deref())
}

/// (text, error, copy offered) for the statusLine row. `command_supported`
/// is false when AUM's own path cannot be put into a shell command safely;
/// then nothing is offered for copying. `store_build` picks how live updates
/// are turned on: the Settings toggle (MSIX) or `--enable-claude-bridge`
/// (EXE builds, which have no toggle). Another tool's status line is
/// described generically; its program is never named in the UI.
fn statusline_row(
    state: &claude_statusline::StatusLineState,
    labels: &localization::SettingsStrings,
    bridge_wanted: bool,
    command_supported: bool,
    store_build: bool,
) -> (String, bool, bool) {
    use claude_statusline::StatusLineState as State;
    match state {
        State::ConfiguredForAum if bridge_wanted => (labels.statusline_aum.into(), false, false),
        State::ConfiguredForAum if store_build => {
            (labels.statusline_aum_inactive.into(), true, false)
        }
        State::ConfiguredForAum => (labels.statusline_aum_inactive_exe.into(), true, false),
        State::NotConfigured | State::SettingsNotFound if !command_supported => {
            (labels.statusline_path_unsupported.into(), true, false)
        }
        State::NotConfigured => (labels.statusline_free.into(), false, true),
        State::SettingsNotFound => (labels.statusline_no_settings.into(), false, true),
        State::OtherCommand { .. } => (labels.statusline_other.into(), true, false),
        State::Unreadable => (labels.statusline_unreadable.into(), true, false),
    }
}

fn apply_statusline_row(window: &SettingsWindow) {
    let Some((locale, bridge_wanted)) = STATUSLINE_CONTEXT.lock().ok().and_then(|c| *c) else {
        return;
    };
    let labels = locale.settings();
    window.set_statusline_label(labels.statusline_title.into());
    window.set_statusline_copy_label(labels.statusline_copy.into());
    let state = STATUSLINE_STATE.lock().ok().and_then(|state| state.clone());
    let Some(state) = state else {
        window.set_statusline_text("".into());
        window.set_statusline_can_copy(false);
        return;
    };
    let (text, error, can_copy) = statusline_row(
        &state,
        &labels,
        bridge_wanted,
        bridge_command().is_ok(),
        packaged::is_packaged(),
    );
    window.set_statusline_text(text.into());
    window.set_statusline_error(error);
    window.set_statusline_can_copy(can_copy);
}

/// Native window handle of a Slint window (0 when unavailable).
fn native_window_handle(window: &slint::Window) -> isize {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let handle = window.window_handle();
        if let Ok(handle) = handle.window_handle()
            && let RawWindowHandle::Win32(handle) = handle.as_raw()
        {
            return handle.hwnd.get();
        }
        0
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        0
    }
}

/// Copies the AUM `statusLine` setting for the user to paste themselves. The
/// button shows the actual result ("Copied" or "Copy failed") for 2 s.
fn copy_statusline_setting(window: &SettingsWindow) {
    let result = bridge_command()
        .map_err(|error| format!("{error:?}"))
        .and_then(|command| {
            claude_statusline::copy_to_clipboard(
                native_window_handle(window.window()),
                &claude_statusline::setting_snippet(&command),
            )
            .map_err(|error| format!("{error:?}"))
        });
    match &result {
        Ok(()) => eprintln!("STATUSLINE: setting copied to the clipboard"),
        Err(error) => eprintln!("STATUSLINE: copy failed: {error}"),
    }
    if let Some((locale, _)) = STATUSLINE_CONTEXT.lock().ok().and_then(|c| *c) {
        let labels = locale.settings();
        let label = if result.is_ok() {
            labels.statusline_copied
        } else {
            labels.statusline_copy_failed
        };
        window.set_statusline_copy_label(label.into());
    }
    let weak = window.as_weak();
    slint::Timer::single_shot(Duration::from_secs(2), move || {
        if let Some(window) = weak.upgrade() {
            apply_statusline_row(&window);
        }
    });
}

fn show_settings_centered(window: &SettingsWindow) {
    refresh_statusline_state();
    apply_statusline_row(window);
    // MSIX: the user may have changed the startup task in Windows meanwhile;
    // re-read it off the UI thread (the toggle waits until the result is in).
    if packaged::is_packaged()
        && autostart::packaged_controller().submit(autostart::StartupRequest::Refresh)
    {
        window.set_startup_busy(true);
    }
    let _ = window.show();
    activate_window(window.window());
    let mode = match window.get_theme_kind() {
        1 => ThemeMode::BluishDark,
        2 => ThemeMode::Light,
        _ => ThemeMode::Graphite,
    };
    if !theme::apply_title_bar(window.window(), mode) {
        retry_title_bar(window.as_weak(), mode, 20);
    }
    place_settings(window);
    // The final scale factor can arrive a few event-loop turns after show()
    // (per-monitor DPI); place once more with it.
    let weak = window.as_weak();
    slint::Timer::single_shot(Duration::from_millis(150), move || {
        if let Some(window) = weak.upgrade() {
            place_settings(&window);
        }
    });
}

/// Requests ordinary foreground activation for a window opened by an explicit
/// tray/menu action. The OS remains responsible for its normal foreground lock.
fn activate_window(window: &slint::Window) {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
        let handle = window.window_handle();
        let Ok(handle) = handle.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        unsafe {
            let _ = SetForegroundWindow(handle.hwnd.get() as _);
        }
    }
    #[cfg(not(windows))]
    {
        use slint::winit_030::{WinitWindowAccessor, winit::window::WindowLevel};
        window.with_winit_window(|native| {
            native.set_window_level(WindowLevel::Normal);
            native.focus_window();
        });
    }
}

/// Default size at the window's scale, clamped to the work area of the
/// monitor under the cursor (outer frame included), centred there.
fn place_settings(window: &SettingsWindow) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut cursor = POINT { x: 0, y: 0 };
        let mut monitor_info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: Default::default(),
            rcWork: Default::default(),
            dwFlags: 0,
        };
        unsafe {
            if GetCursorPos(&mut cursor) != 0 {
                let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
                if !monitor.is_null() && GetMonitorInfoW(monitor, &mut monitor_info) != 0 {
                    let area = monitor_info.rcWork;
                    // Keep the whole window inside the work area on small
                    // screens or high DPI; the content scrolls when shortened.
                    // `size()` is the client area; reserve the native title bar
                    // (~32 logical px) so the outer window also fits.
                    let scale = window.window().scale_factor();
                    let margin = (16.0 * scale).round() as i32;
                    let caption = (32.0 * scale).round() as i32;
                    let max_width = (area.right - area.left - 2 * margin).max(1) as u32;
                    let max_height = (area.bottom - area.top - 2 * margin - caption).max(1) as u32;
                    // Open at the default size (fits the whole About page)
                    // at the window's actual scale, never larger than the
                    // work area. `size()` can still report the pre-show size
                    // here, so it is computed from the logical default.
                    let logical = slint::LogicalSize::new(
                        window.get_default_width().min(max_width as f32 / scale),
                        window.get_default_height().min(max_height as f32 / scale),
                    );
                    window.window().set_size(logical);
                    let size = logical.to_physical(scale);
                    let x = area.left + ((area.right - area.left - size.width as i32) / 2);
                    let y = area.top
                        + ((area.bottom - area.top - size.height as i32 - caption) / 2).max(0);
                    window
                        .window()
                        .set_position(slint::PhysicalPosition::new(x, y));
                }
            }
        }
    }
}

/// The native window can be created a few event-loop turns after show();
/// retry the title-bar coloring briefly until the handle exists.
fn retry_title_bar(window: slint::Weak<SettingsWindow>, mode: ThemeMode, attempts: u8) {
    if attempts == 0 {
        return;
    }
    slint::Timer::single_shot(Duration::from_millis(50), move || {
        if let Some(panel) = window.upgrade() {
            if !theme::apply_title_bar(panel.window(), mode) {
                retry_title_bar(panel.as_weak(), mode, attempts - 1);
            }
        }
    });
}

fn open_repository() {
    open_url("https://github.com/valenticus/agent-usage-monitor");
}

fn open_issues() {
    open_url("https://github.com/valenticus/agent-usage-monitor/issues");
}

fn open_url(address: &str) {
    #[cfg(not(windows))]
    let _ = address;
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        let operation: Vec<u16> = "open\0".encode_utf16().collect();
        let url: Vec<u16> = address.encode_utf16().chain(std::iter::once(0)).collect();
        let _ = ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        );
    }
}

#[cfg(windows)]
fn apply_popup_taskbar_style(window: &MainWindow) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };
    let window_handle = window.window().window_handle();
    let Ok(handle) = window_handle.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return false;
    };
    let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let updated = (style | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW;
        if style != updated {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, updated as isize);
            let _ = SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
            );
        }
    }
    true
}

fn system_light(settings: &AppSettings) -> bool {
    match settings.theme_mode {
        ThemeMode::Graphite | ThemeMode::BluishDark => false,
        ThemeMode::Light => true,
    }
}

fn resolve_locale(settings: &AppSettings) -> Locale {
    Locale::resolve(settings.language)
}
fn lock_settings(settings: &SharedSettings) -> AppSettings {
    settings
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default()
}
fn lock_locale(locale: &SharedLocale) -> Locale {
    locale.lock().map(|value| *value).unwrap_or(Locale::En)
}
fn valid_settings_tab(tab: i32) -> bool {
    (0..=2).contains(&tab)
}

fn set_theme(settings: &mut AppSettings, value: &str, locale: Locale) {
    let labels = locale.settings();
    settings.theme_mode = if value == labels.graphite {
        ThemeMode::Graphite
    } else if value == labels.navy {
        ThemeMode::BluishDark
    } else if value == labels.light {
        ThemeMode::Light
    } else {
        ThemeMode::Graphite
    };
}
fn set_language(settings: &mut AppSettings, value: &str, _locale: Locale) {
    settings.language = if value == "Українська" {
        Language::Ukrainian
    } else if value == "English" {
        Language::English
    } else if value == "Polski" {
        Language::Polish
    } else if value == "Русский" {
        Language::Russian
    } else {
        return;
    };
}
fn toggle_codex(settings: &mut AppSettings) {
    settings.codex_enabled = !settings.codex_enabled;
}
fn toggle_claude(settings: &mut AppSettings) {
    settings.claude_enabled = !settings.claude_enabled;
}
fn set_warning_threshold(settings: &mut AppSettings, threshold: u8) {
    if threshold >= settings.critical_threshold && threshold <= 100 {
        settings.warning_threshold = threshold;
    }
}
fn set_critical_threshold(settings: &mut AppSettings, threshold: u8) {
    if threshold <= settings.warning_threshold {
        settings.critical_threshold = threshold;
    }
}
fn set_notification_duration(settings: &mut AppSettings, seconds: u16) {
    if (1..=120).contains(&seconds) {
        settings.popup_duration_seconds = seconds;
    }
}

fn status_for(error: &ProviderError) -> ProviderStatus {
    if error.code == "codex_unavailable" {
        ProviderStatus::Unavailable
    } else {
        ProviderStatus::Error
    }
}
fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_legend_matches_scale_order_and_uses_configured_thresholds() {
        let uk = threshold_legend(&Locale::Uk.settings(), 30, 15);
        assert_eq!(
            uk,
            [
                "Критично до 15%",
                "Попередження 15\u{2060}–\u{2060}30%",
                "Норма понад 30%"
            ]
        );
        let en = threshold_legend(&Locale::En.settings(), 25, 10);
        assert_eq!(
            en,
            [
                "Critical up to 10%",
                "Warning 10\u{2060}–\u{2060}25%",
                "Normal above 25%"
            ]
        );
        for locale in [Locale::Uk, Locale::Pl, Locale::En, Locale::Ru] {
            let legend = threshold_legend(&locale.settings(), 25, 10);
            assert!(legend[0].ends_with(" 10%"));
            assert!(legend[1].ends_with(" 10\u{2060}–\u{2060}25%"));
            // Only the space between label and range is a line-break opportunity.
            assert!(!legend[1].contains(" 10–") && !legend[1].contains("– "));
            assert!(legend[2].ends_with(" 25%"));
        }
    }

    #[test]
    fn repeated_manual_refresh_requests_coalesce_in_bounded_provider_queue() {
        let (sender, mut receiver) = mpsc::channel(1);
        assert!(queue_manual_refresh(
            "claude",
            &sender,
            ManualRefreshRequest {
                id: 1,
                origin: RefreshOrigin::Manual
            }
        ));
        assert!(queue_manual_refresh(
            "claude",
            &sender,
            ManualRefreshRequest {
                id: 2,
                origin: RefreshOrigin::Resume
            }
        ));
        assert_eq!(receiver.try_recv().unwrap().id, 1);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn settings_mutations_preserve_concurrent_independent_updates() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let settings = Arc::new(Mutex::new(AppSettings::default()));
        let provider_settings = settings.clone();
        let provider_path = path.clone();
        let provider = thread::spawn(move || {
            let _mutation = settings_mutation_guard();
            let mut updated = lock_settings(&provider_settings);
            updated.codex_enabled = false;
            settings::save(&provider_path, &updated).unwrap();
            *provider_settings.lock().unwrap() = updated;
        });
        let update_settings = settings.clone();
        let update_path = path.clone();
        let update_checks = thread::spawn(move || {
            let _mutation = settings_mutation_guard();
            let mut updated = lock_settings(&update_settings);
            updated.automatic_update_checks_enabled = true;
            settings::save(&update_path, &updated).unwrap();
            *update_settings.lock().unwrap() = updated;
        });
        provider.join().unwrap();
        update_checks.join().unwrap();

        let actual = settings.lock().unwrap().clone();
        assert!(!actual.codex_enabled);
        assert!(actual.automatic_update_checks_enabled);
        let persisted = settings::load(&path).unwrap();
        assert_eq!(persisted, actual);
    }

    #[test]
    fn resume_refresh_is_skipped_only_after_a_recent_successful_refresh() {
        let now = 1_700_000_000;
        // The overdue tick right after wake-up succeeded: the resume request
        // 10 s later would only repeat it.
        assert!(skip_redundant_resume_refresh(
            RefreshOrigin::Resume,
            Some(now - 10),
            now
        ));
        assert!(skip_redundant_resume_refresh(
            RefreshOrigin::Resume,
            Some(now - (RESUME_COALESCE_SECONDS - 1)),
            now
        ));
        // That tick failed, or data is older (e.g. from before the sleep): run.
        assert!(!skip_redundant_resume_refresh(
            RefreshOrigin::Resume,
            None,
            now
        ));
        assert!(!skip_redundant_resume_refresh(
            RefreshOrigin::Resume,
            Some(now - RESUME_COALESCE_SECONDS),
            now
        ));
        assert!(!skip_redundant_resume_refresh(
            RefreshOrigin::Resume,
            Some(now - 1_200),
            now
        ));
        // A clock step backwards never suppresses the refresh.
        assert!(!skip_redundant_resume_refresh(
            RefreshOrigin::Resume,
            Some(now + 5),
            now
        ));
        // Manual refreshes are never skipped.
        assert!(!skip_redundant_resume_refresh(
            RefreshOrigin::Manual,
            Some(now - 1),
            now
        ));
    }

    #[test]
    fn claude_bridge_listener_needs_claude_and_an_explicit_opt_in() {
        let settings = |claude_enabled, bridge| AppSettings {
            claude_enabled,
            claude_bridge_listener_enabled: bridge,
            ..Default::default()
        };
        // Off by default in both distributions.
        assert!(!claude_bridge_wanted_for(
            false,
            false,
            &AppSettings::default()
        ));
        assert!(!claude_bridge_wanted_for(
            true,
            false,
            &AppSettings::default()
        ));
        // EXE builds: only the command-line opt-in counts; the Store setting is ignored.
        assert!(!claude_bridge_wanted_for(
            false,
            false,
            &settings(true, true)
        ));
        assert!(claude_bridge_wanted_for(
            false,
            true,
            &settings(true, false)
        ));
        // MSIX build: the persisted setting is the opt-in.
        assert!(claude_bridge_wanted_for(true, false, &settings(true, true)));
        assert!(!claude_bridge_wanted_for(
            true,
            false,
            &settings(true, false)
        ));
        // Claude off stops the listener in every build, whatever the opt-in.
        for packaged in [false, true] {
            for argument in [false, true] {
                assert!(!claude_bridge_wanted_for(
                    packaged,
                    argument,
                    &settings(false, true)
                ));
            }
        }
        // Tests never run packaged, so the runtime helper follows the EXE rules.
        assert!(!claude_bridge_wanted(false, &settings(true, true)));
    }

    #[test]
    fn only_confirmed_exe_builds_rewrite_startup_registration_at_launch() {
        use packaged::PackageState;
        // EXE: the HKCU Run value follows the current executable path.
        assert!(launch_reconcile_writes_registration(
            PackageState::Unpackaged
        ));
        // MSIX: the startup task is only read at launch, never enabled silently.
        assert!(!launch_reconcile_writes_registration(
            PackageState::Packaged
        ));
        // Unknown identity: nothing is written.
        assert!(!launch_reconcile_writes_registration(PackageState::Unknown));
        assert_eq!(
            launch_startup_request(PackageState::Packaged),
            Some(autostart::StartupRequest::Refresh)
        );
        assert_eq!(launch_startup_request(PackageState::Unpackaged), None);
        assert_eq!(launch_startup_request(PackageState::Unknown), None);
    }

    #[test]
    fn startup_row_explains_who_controls_sign_in_launch() {
        use autostart::{StartupControl, StartupStatus};
        let ok = |enabled, control| Ok(StartupStatus { enabled, control });
        // Not read yet, or a request in flight: wait, keep the last value.
        assert_eq!(
            startup_row(None, false),
            StartupRow {
                enabled: false,
                interactive: false,
                busy: true,
                text: StartupText::Checking
            }
        );
        let row = startup_row(Some(&ok(true, StartupControl::App)), true);
        assert!(row.busy && row.enabled && !row.interactive);
        assert_eq!(row.text, StartupText::Checking);
        // App-controlled: operable.
        let row = startup_row(Some(&ok(false, StartupControl::App)), false);
        assert!(row.interactive && !row.enabled && !row.busy);
        assert_eq!(row.text, StartupText::Normal);
        // Locked by the user in Windows or by policy: shown as is, not operable.
        let row = startup_row(Some(&ok(false, StartupControl::DisabledInWindows)), false);
        assert!(!row.interactive && !row.enabled);
        assert_eq!(row.text, StartupText::WindowsControlled);
        for enabled in [false, true] {
            let row = startup_row(Some(&ok(enabled, StartupControl::Policy)), false);
            assert!(!row.interactive);
            assert_eq!(row.enabled, enabled);
            assert_eq!(row.text, StartupText::Policy);
        }
        // Failure: reported, not operable, shown off.
        assert_eq!(startup_row_error().text, StartupText::Error);
        assert!(!startup_row_error().interactive && !startup_row_error().enabled);
        for locale in [Locale::Uk, Locale::En, Locale::Pl, Locale::Ru] {
            let labels = locale.settings();
            assert_eq!(
                startup_text(&labels, StartupText::Normal),
                labels.startup_description
            );
            assert_eq!(
                startup_text(&labels, StartupText::WindowsControlled),
                labels.startup_windows_controlled
            );
            assert_eq!(
                startup_text(&labels, StartupText::Policy),
                labels.startup_policy_controlled
            );
            assert_eq!(
                startup_text(&labels, StartupText::Error),
                labels.startup_error
            );
            assert_eq!(
                startup_text(&labels, StartupText::Checking),
                labels.startup_checking
            );
        }
    }

    /// Fake status-line listener: counts starts, how many run right now (a
    /// drop guard notices when the lifecycle drops the future) and the peak.
    #[derive(Default)]
    struct ListenerProbe {
        starts: std::sync::atomic::AtomicUsize,
        running: std::sync::atomic::AtomicUsize,
        peak: std::sync::atomic::AtomicUsize,
        fail_next: std::sync::atomic::AtomicBool,
    }

    struct RunningGuard(Arc<ListenerProbe>);
    impl Drop for RunningGuard {
        fn drop(&mut self) {
            self.0.running.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl ListenerProbe {
        fn listener(
            probe: Arc<ListenerProbe>,
        ) -> impl FnMut(
            CancellationToken,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = std::io::Result<()>> + Send>,
        > {
            let probe = probe.clone();
            move |token| {
                let probe = probe.clone();
                Box::pin(async move {
                    probe.starts.fetch_add(1, Ordering::SeqCst);
                    let now = probe.running.fetch_add(1, Ordering::SeqCst) + 1;
                    probe.peak.fetch_max(now, Ordering::SeqCst);
                    let _guard = RunningGuard(probe.clone());
                    if probe.fail_next.swap(false, Ordering::SeqCst) {
                        return Err(std::io::Error::other("pipe busy"));
                    }
                    token.cancelled().await;
                    Ok(())
                })
            }
        }
        fn starts(&self) -> usize {
            self.starts.load(Ordering::SeqCst)
        }
        fn running(&self) -> usize {
            self.running.load(Ordering::SeqCst)
        }
    }

    async fn settle() {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }

    fn store_settings(claude_enabled: bool, bridge: bool) -> AppSettings {
        AppSettings {
            claude_enabled,
            claude_bridge_listener_enabled: bridge,
            ..Default::default()
        }
    }

    /// The Store-build rule, independent of this process's identity.
    fn store_rule(settings: &AppSettings) -> bool {
        claude_bridge_wanted_for(true, false, settings)
    }

    #[tokio::test]
    async fn bridge_listener_follows_claude_and_the_opt_in() {
        let probe = Arc::new(ListenerProbe::default());
        let failures = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (settings, receiver) = watch::channel(AppSettings::default());
        let cancellation = CancellationToken::new();
        let failure_count = failures.clone();
        let lifecycle = tokio::spawn(run_bridge_lifecycle(
            receiver,
            store_rule,
            cancellation.clone(),
            ListenerProbe::listener(probe.clone()),
            move || {
                failure_count.fetch_add(1, Ordering::SeqCst);
            },
        ));
        // Off by default.
        settle().await;
        assert_eq!((probe.starts(), probe.running()), (0, 0));
        // Enable the bridge.
        settings.send(store_settings(true, true)).unwrap();
        settle().await;
        assert_eq!((probe.starts(), probe.running()), (1, 1));
        // Disable Claude: the listener stops.
        settings.send(store_settings(false, true)).unwrap();
        settle().await;
        assert_eq!((probe.starts(), probe.running()), (1, 0));
        // Re-enable Claude: the opt-in was never revoked, so it runs again.
        settings.send(store_settings(true, true)).unwrap();
        settle().await;
        assert_eq!((probe.starts(), probe.running()), (2, 1));
        // Disable the bridge: the listener stops.
        settings.send(store_settings(true, false)).unwrap();
        settle().await;
        assert_eq!((probe.starts(), probe.running()), (2, 0));
        // Re-enabling Claude alone never starts it without the opt-in.
        settings.send(store_settings(false, false)).unwrap();
        settings.send(store_settings(true, false)).unwrap();
        settle().await;
        assert_eq!((probe.starts(), probe.running()), (2, 0));
        // Rapid toggles: never more than one listener at a time, and the final
        // state wins.
        for index in 0..50 {
            settings
                .send(store_settings(index % 3 != 0, index % 2 == 0))
                .unwrap();
            if index % 7 == 0 {
                settle().await;
            }
        }
        settings.send(store_settings(true, true)).unwrap();
        settle().await;
        assert_eq!(probe.running(), 1);
        assert_eq!(probe.peak.load(Ordering::SeqCst), 1);
        // Shutdown while the listener runs: stops and never restarts.
        cancellation.cancel();
        tokio::time::timeout(Duration::from_secs(2), lifecycle)
            .await
            .expect("lifecycle stops on cancellation")
            .unwrap();
        assert_eq!(probe.running(), 0);
        assert_eq!(failures.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn failed_bridge_listener_is_reported_once_and_restarted_only_after_a_toggle() {
        let probe = Arc::new(ListenerProbe::default());
        probe.fail_next.store(true, Ordering::SeqCst);
        let failures = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (settings, receiver) = watch::channel(store_settings(true, true));
        let cancellation = CancellationToken::new();
        let failure_count = failures.clone();
        let lifecycle = tokio::spawn(run_bridge_lifecycle(
            receiver,
            store_rule,
            cancellation.clone(),
            ListenerProbe::listener(probe.clone()),
            move || {
                failure_count.fetch_add(1, Ordering::SeqCst);
            },
        ));
        settle().await;
        // One attempt, one report, no busy retry loop.
        assert_eq!((probe.starts(), probe.running()), (1, 0));
        assert_eq!(failures.load(Ordering::SeqCst), 1);
        // An unrelated settings change does not retry.
        settings
            .send(AppSettings {
                warning_threshold: 30,
                ..store_settings(true, true)
            })
            .unwrap();
        settle().await;
        assert_eq!(probe.starts(), 1);
        // Turning the bridge off and on retries.
        settings.send(store_settings(true, false)).unwrap();
        settle().await;
        settings.send(store_settings(true, true)).unwrap();
        settle().await;
        assert_eq!((probe.starts(), probe.running()), (2, 1));
        assert_eq!(failures.load(Ordering::SeqCst), 1);
        cancellation.cancel();
        tokio::time::timeout(Duration::from_secs(2), lifecycle)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(probe.running(), 0);
    }

    #[tokio::test]
    async fn closed_settings_channel_stops_the_bridge_on_shutdown() {
        let probe = Arc::new(ListenerProbe::default());
        let (settings, receiver) = watch::channel(store_settings(true, true));
        let cancellation = CancellationToken::new();
        let lifecycle = tokio::spawn(run_bridge_lifecycle(
            receiver,
            store_rule,
            cancellation.clone(),
            ListenerProbe::listener(probe.clone()),
            || {},
        ));
        settle().await;
        assert_eq!(probe.running(), 1);
        // The UI dropped its sender (shutting down): keep running until the
        // app cancels, then stop.
        drop(settings);
        settle().await;
        cancellation.cancel();
        tokio::time::timeout(Duration::from_secs(2), lifecycle)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(probe.running(), 0);
        assert_eq!(probe.starts(), 1);
    }

    /// F-8 guided: every statusLine state maps to one honest message; "copy"
    /// is offered only when the status line is free, never over another
    /// tool's command; another tool is named by program only.
    #[test]
    fn statusline_row_states_and_copy_offer() {
        use claude_statusline::StatusLineState as State;
        for locale in [Locale::Uk, Locale::En, Locale::Pl, Locale::Ru] {
            let labels = locale.settings();
            for store in [true, false] {
                let row =
                    |state: State, bridge| statusline_row(&state, &labels, bridge, true, store);
                // Configured and receiving: confirmation, nothing to copy.
                assert_eq!(
                    row(State::ConfiguredForAum, true),
                    (labels.statusline_aum.into(), false, false)
                );
                // Configured but live updates off: says how to turn them on
                // in this build (toggle in the Store build, flag in EXE builds).
                let (text, error, copy) = row(State::ConfiguredForAum, false);
                assert!(error && !copy);
                assert_eq!(
                    text.contains("--enable-claude-bridge"),
                    !store,
                    "{locale:?}"
                );
                if store {
                    assert!(
                        text.contains(labels.claude_bridge),
                        "{locale:?}: names the toggle"
                    );
                }
                // Free or missing settings: manual setup with the copy offer,
                // whether or not the live updates are on.
                for bridge in [true, false] {
                    for state in [State::NotConfigured, State::SettingsNotFound] {
                        let (text, error, copy) = row(state, bridge);
                        assert!(!error && copy && text.contains("~/.claude/settings.json"));
                    }
                }
                assert_eq!(
                    row(State::Unreadable, true),
                    (labels.statusline_unreadable.into(), true, false)
                );
                // Another command: one generic text, no program name, never
                // offered for replacement.
                for program in ["nook-hook-standalone.exe", "", "ccline"] {
                    let (text, error, copy) = row(
                        State::OtherCommand {
                            program: program.into(),
                        },
                        true,
                    );
                    assert_eq!(text, labels.statusline_other, "{locale:?}");
                    assert!(
                        error && !copy,
                        "{locale:?}: a foreign status line is never offered for replacement"
                    );
                }
                // AUM's own path refused for a shell command: nothing to copy.
                for state in [State::NotConfigured, State::SettingsNotFound] {
                    assert_eq!(
                        statusline_row(&state, &labels, true, false, store),
                        (labels.statusline_path_unsupported.into(), true, false)
                    );
                }
            }
            // No UI string names a third-party tool or leaves a placeholder.
            for text in [
                labels.claude_note,
                labels.claude_refresh_description,
                labels.claude_bridge,
                labels.claude_bridge_description,
                labels.statusline_title,
                labels.statusline_aum,
                labels.statusline_aum_inactive,
                labels.statusline_aum_inactive_exe,
                labels.statusline_free,
                labels.statusline_other,
                labels.statusline_no_settings,
                labels.statusline_unreadable,
            ] {
                let lower = text.to_lowercase();
                assert!(
                    !lower.contains("nook") && !text.contains('{'),
                    "{locale:?}: {text}"
                );
            }
            // The toggle never claims to configure Claude Code: its
            // description and the free-state text both say setup is manual.
            assert!(labels.claude_bridge_description.contains("Claude Code"));
            assert!(labels.statusline_free.contains("~/.claude/settings.json"));
        }
    }

    #[test]
    fn only_missing_cli_maps_to_unavailable() {
        assert_eq!(
            status_for(&ProviderError {
                code: "codex_unavailable".into(),
                message: String::new()
            }),
            ProviderStatus::Unavailable
        );
        assert_eq!(
            status_for(&ProviderError {
                code: "codex_timeout".into(),
                message: String::new()
            }),
            ProviderStatus::Error
        );
    }

    #[test]
    fn explicit_theme_modes_resolve_without_system_theme_ambiguity() {
        assert!(!system_light(&AppSettings {
            theme_mode: ThemeMode::Graphite,
            ..Default::default()
        }));
        assert!(!system_light(&AppSettings {
            theme_mode: ThemeMode::BluishDark,
            ..Default::default()
        }));
        assert!(system_light(&AppSettings {
            theme_mode: ThemeMode::Light,
            ..Default::default()
        }));
    }

    #[test]
    fn settings_navigation_uses_exactly_three_valid_tabs() {
        assert!(valid_settings_tab(0));
        assert!(valid_settings_tab(1));
        assert!(valid_settings_tab(2));
        assert!(!valid_settings_tab(-1));
        assert!(!valid_settings_tab(3));
    }

    #[test]
    fn appearance_combos_select_persistable_theme_and_language_values() {
        let mut settings = AppSettings::default();
        set_theme(&mut settings, "Графітова", Locale::Uk);
        assert_eq!(settings.theme_mode, ThemeMode::Graphite);
        set_theme(&mut settings, "Синювата темна", Locale::Uk);
        assert_eq!(settings.theme_mode, ThemeMode::BluishDark);
        set_theme(&mut settings, "Світла", Locale::Uk);
        assert_eq!(settings.theme_mode, ThemeMode::Light);

        set_language(&mut settings, "Українська", Locale::En);
        assert_eq!(settings.language, Language::Ukrainian);
        set_language(&mut settings, "English", Locale::Uk);
        assert_eq!(settings.language, Language::English);
        set_language(&mut settings, "Polski", Locale::En);
        assert_eq!(settings.language, Language::Polish);
        set_language(&mut settings, "Русский", Locale::En);
        assert_eq!(settings.language, Language::Russian);
    }

    #[test]
    fn notification_threshold_controls_preserve_critical_less_than_warning() {
        let mut settings = AppSettings::default();
        set_critical_threshold(&mut settings, 30);
        assert_eq!(settings.critical_threshold, 10);
        set_warning_threshold(&mut settings, 40);
        set_critical_threshold(&mut settings, 30);
        assert_eq!(
            (settings.warning_threshold, settings.critical_threshold),
            (40, 30)
        );
        set_warning_threshold(&mut settings, 20);
        assert_eq!(settings.warning_threshold, 40);
        set_notification_duration(&mut settings, 8);
        assert_eq!(settings.popup_duration_seconds, 8);
        set_notification_duration(&mut settings, 0);
        assert_eq!(settings.popup_duration_seconds, 8);
    }

    #[test]
    fn provider_switches_round_trip_independently_through_settings_storage() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut settings = AppSettings::default();
        toggle_codex(&mut settings);
        toggle_claude(&mut settings);
        settings::save(&path, &settings).unwrap();
        let loaded = settings::load(&path).unwrap();
        assert!(!loaded.codex_enabled);
        assert!(!loaded.claude_enabled);
        assert!(loaded.alerts_enabled);
    }

    #[test]
    fn alert_balloon_copy_is_localized_and_uses_real_remaining_values() {
        let event = AlertEvent {
            key: agent_usage_monitor_core::AlertKey {
                provider_id: "claude".into(),
                window: AlertWindow::Weekly,
            },
            severity: agent_usage_monitor_core::AlertSeverity::Critical,
            remaining_percent: 8,
            reset_at: None,
            popup_duration_seconds: 8,
        };
        let (title, message) = alert_notification_copy(&event, Locale::Uk);
        assert!(title.contains("Claude"));
        assert!(title.contains("тижневий ліміт"));
        assert_eq!(message, "Залишилося 8%");
        let (title, message) = alert_notification_copy(&event, Locale::Pl);
        assert!(title.contains("limit tygodniowy"));
        assert_eq!(message, "Pozostało 8%");
    }

    #[test]
    fn provider_selection_can_represent_all_four_valid_combinations() {
        for (codex_enabled, claude_enabled) in
            [(true, true), (true, false), (false, true), (false, false)]
        {
            let settings = AppSettings {
                codex_enabled,
                claude_enabled,
                ..Default::default()
            };
            assert_eq!(settings.codex_enabled, codex_enabled);
            assert_eq!(settings.claude_enabled, claude_enabled);
        }
    }
}
