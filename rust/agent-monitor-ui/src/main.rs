// Release builds are GUI-subsystem apps (no console window). Debug builds
// keep the console for the diagnostic log lines.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adapter;
mod aum_icon;
mod autostart;
mod claude_refresher;
mod claude_statusline;
mod flyout;
mod installer_migration;
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

use std::cell::RefCell;
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
static QA_ISOLATED: AtomicBool = AtomicBool::new(false);
#[cfg(any(debug_assertions, test))]
static QA_ABOUT_PREVIEW: std::sync::Mutex<Option<AboutPreview>> = std::sync::Mutex::new(None);
#[cfg(debug_assertions)]
static QA_GEOMETRY: AtomicBool = AtomicBool::new(false);
static SETTINGS_OPEN_PENDING: AtomicBool = AtomicBool::new(false);
static UPDATE_STATUS: std::sync::Mutex<UpdateStatus> = std::sync::Mutex::new(UpdateStatus::Idle);
static SETTINGS_MUTATION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static SETTINGS_LAYOUT: std::sync::Mutex<SettingsLayoutSnapshot> =
    std::sync::Mutex::new(SettingsLayoutSnapshot::new());
thread_local! {
    static PREVIEW_WINDOW: RefCell<Option<slint::Weak<MainWindow>>> = const { RefCell::new(None) };
}

const SETTINGS_MARGIN_LOGICAL: f32 = 16.0;
const SETTINGS_CAPTION_LOGICAL: f32 = 32.0;
const SETTINGS_MIN_HEIGHT_LOGICAL: f32 = 420.0;

#[cfg(any(debug_assertions, test))]
#[derive(Clone, Copy)]
struct AboutPreview {
    distribution: Distribution,
    status: AboutPreviewStatus,
}

#[cfg(any(debug_assertions, test))]
#[derive(Debug, Clone, Copy)]
enum AboutPreviewStatus {
    Idle,
    Checking,
    Available,
    Current,
    Long,
    Offline,
    Http,
    Invalid,
    Timeout,
}

#[cfg(any(debug_assertions, test))]
impl AboutPreview {
    fn from_args(args: &[String]) -> Option<Self> {
        let distribution = match args
            .iter()
            .find_map(|arg| arg.strip_prefix("--qa-about-distribution="))
        {
            Some("portable") => Distribution::GitHubPortable,
            Some("installer") => Distribution::GitHubInstaller,
            Some("store") => Distribution::MicrosoftStore,
            _ => return None,
        };
        let status = match args
            .iter()
            .find_map(|arg| arg.strip_prefix("--qa-about-status="))
        {
            Some("idle") => AboutPreviewStatus::Idle,
            Some("available") => AboutPreviewStatus::Available,
            Some("checking") => AboutPreviewStatus::Checking,
            Some("current") => AboutPreviewStatus::Current,
            Some("long") => AboutPreviewStatus::Long,
            Some("offline") => AboutPreviewStatus::Offline,
            Some("http") => AboutPreviewStatus::Http,
            Some("invalid") => AboutPreviewStatus::Invalid,
            Some("timeout") => AboutPreviewStatus::Timeout,
            None => AboutPreviewStatus::Idle,
            _ => return None,
        };
        Some(Self {
            distribution,
            status,
        })
    }
}

#[cfg(any(debug_assertions, test))]
fn about_display_distribution(
    production_distribution: Distribution,
    qa_isolated: bool,
    preview: Option<AboutPreview>,
) -> Distribution {
    if qa_isolated {
        preview
            .map(|preview| preview.distribution)
            .unwrap_or(production_distribution)
    } else {
        production_distribution
    }
}

fn update_checks_allowed(qa_isolated: bool, distribution: Distribution) -> bool {
    !qa_isolated && distribution.can_offer_github_update()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PhysicalWorkArea {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SettingsScreenSignature {
    monitor: usize,
    work_area: PhysicalWorkArea,
    scale_bits: u32,
}

#[derive(Debug, Clone, Copy, Default)]
struct SettingsLayoutSnapshot {
    screen: Option<SettingsScreenSignature>,
    size_ownership: SettingsSizeOwnership,
}

impl SettingsLayoutSnapshot {
    const fn new() -> Self {
        Self {
            screen: None,
            size_ownership: SettingsSizeOwnership::new(),
        }
    }
}

/// Tracks whether the Settings size was chosen by the user. Only a native
/// modal size loop that contains `WM_SIZING` counts: Windows sends
/// `WM_SIZING` solely while the user drags a border or sizes the window from
/// the keyboard. App-requested `SetWindowPos` resizes (including Winit's
/// asynchronous `set_size`), DPI changes, maximize and snap never send it, so
/// none of them turn an automatic size into a retained one.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct SettingsSizeOwnership {
    in_size_move: bool,
    user_sizing_seen: bool,
    user_size: Option<(f32, f32)>,
}

impl SettingsSizeOwnership {
    const fn new() -> Self {
        Self {
            in_size_move: false,
            user_sizing_seen: false,
            user_size: None,
        }
    }

    /// `WM_ENTERSIZEMOVE`: a native move or size loop starts.
    fn enter_size_move(&mut self) {
        self.in_size_move = true;
        self.user_sizing_seen = false;
    }

    /// `WM_SIZING`: the user is changing the size inside that loop.
    fn user_sizing(&mut self) {
        if self.in_size_move {
            self.user_sizing_seen = true;
        }
    }

    /// `WM_EXITSIZEMOVE` with the final logical client size. Returns the
    /// size recorded as user-selected, if this loop resized the window.
    fn exit_size_move(&mut self, client_logical: (f32, f32)) -> Option<(f32, f32)> {
        let user_resized = self.in_size_move && self.user_sizing_seen;
        self.in_size_move = false;
        self.user_sizing_seen = false;
        let valid = client_logical.0.is_finite()
            && client_logical.1.is_finite()
            && client_logical.0 > 0.0
            && client_logical.1 > 0.0;
        if user_resized && valid {
            self.user_size = Some(client_logical);
            return self.user_size;
        }
        None
    }

    fn user_size(&self) -> Option<(f32, f32)> {
        self.user_size
    }
}

/// Which monitor bounds a Settings geometry pass. A hidden window is opened
/// on the monitor under the cursor; a visible window is fitted on the monitor
/// that contains it, so a post-show remeasure cannot follow the mouse to
/// another display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsMonitor {
    Cursor,
    Window,
}

fn settings_fit_monitor(window_visible: bool) -> SettingsMonitor {
    if window_visible {
        SettingsMonitor::Window
    } else {
        SettingsMonitor::Cursor
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SettingsGeometry {
    width: f32,
    height: f32,
    minimum_height: f32,
    x: i32,
    y: i32,
}

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
    let args: Vec<String> = std::env::args().collect();
    #[cfg(debug_assertions)]
    let qa_isolated = args.iter().any(|arg| arg == "--qa-isolated");
    #[cfg(not(debug_assertions))]
    let qa_isolated = false;
    QA_ISOLATED.store(qa_isolated, Ordering::Release);
    #[cfg(debug_assertions)]
    QA_GEOMETRY.store(
        qa_isolated && args.iter().any(|arg| arg == "--qa-geometry"),
        Ordering::Release,
    );

    #[cfg(windows)]
    let _instance_guard = match SingleInstance::acquire(qa_isolated) {
        Ok(Some(guard)) => guard,
        Ok(None) => return Ok(()),
        Err(()) => {
            eprintln!("Could not establish the single-instance guard.");
            return Ok(());
        }
    };
    // App identity for toasts (sender name + static AUM icon); per-user only.
    #[cfg(windows)]
    if !qa_isolated && let Err(error) = notifications::register_identity() {
        eprintln!("NOTIFY: app identity registration failed: {error}");
    }
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
    if qa_isolated && (!empty_mode || live_mode || demo_mode || claude_bridge_enabled) {
        eprintln!("--qa-isolated requires --empty and cannot use live integrations.");
        return Ok(());
    }
    #[cfg(debug_assertions)]
    if qa_isolated {
        let preview = AboutPreview::from_args(&args);
        let has_about_preview_args = args.iter().any(|arg| {
            arg.starts_with("--qa-about-distribution=") || arg.starts_with("--qa-about-status=")
        });
        if has_about_preview_args {
            let Some(preview) = preview else {
                eprintln!("Invalid About QA preview arguments.");
                return Ok(());
            };
            if let Ok(mut current) = QA_ABOUT_PREVIEW.lock() {
                *current = Some(preview);
            }
        }
    }
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
    let loaded_settings_result = settings::load(&settings_path);
    let migration_can_persist = loaded_settings_result.is_ok() || !settings_path.exists();
    let mut loaded_settings = loaded_settings_result.unwrap_or_default();
    let mut autostart_reconciliation_allowed = migration_can_persist;
    if !qa_isolated && migration_can_persist && packaged::is_confirmed_unpackaged() {
        match installer_migration::consume_pending(&mut loaded_settings, &settings_path) {
            Ok(true) => eprintln!("INSTALLER_MIGRATION: completed legacy bridge opt-in migration"),
            Ok(false) => {}
            Err(error) => {
                eprintln!("INSTALLER_MIGRATION: migration remains pending: {error}");
                autostart_reconciliation_allowed = false;
            }
        }
    }
    let config: SharedSettings = Arc::new(Mutex::new(loaded_settings));
    if !qa_isolated && autostart_reconciliation_allowed {
        reconcile_autostart(&config, &settings_path);
    }
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
    let preview_window = MainWindow::new()?;
    preview_window.set_compact(true);
    PREVIEW_WINDOW.with(|preview| *preview.borrow_mut() = Some(preview_window.as_weak()));
    let settings_window = SettingsWindow::new()?;
    window.set_refresh_enabled(use_live);
    preview_window.set_refresh_enabled(use_live);
    settings_window.set_window_icon(theme::aum_icon_image(64));
    adapter::apply_config(
        &window,
        &initial_snapshot,
        now_unix(),
        &lock_settings(&config),
        lock_locale(&locale),
        system_light(&lock_settings(&config)),
    );
    adapter::apply_config(
        &preview_window,
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
    #[cfg(debug_assertions)]
    let update_display_distribution = about_display_distribution(
        update_distribution,
        qa_isolated,
        QA_ABOUT_PREVIEW.lock().ok().and_then(|preview| *preview),
    );
    #[cfg(not(debug_assertions))]
    let update_display_distribution = update_distribution;

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
            // A user-selected size was already recorded when its native size
            // loop ended; an automatic size is deliberately not retained.
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
                    // Tab changes reset only the active page's scroll position.
                    panel.invoke_select_settings_tab(tab);
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
    if !qa_isolated {
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
    } else {
        settings_window.set_startup_state_known(false);
        settings_window.set_startup_busy(false);
        settings_window.set_startup_enabled(false);
    }
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
    settings_window.on_open_repository(move || {
        if !qa_isolated {
            open_repository();
        }
    });
    settings_window.on_open_issues(move || {
        if !qa_isolated {
            open_issues();
        }
    });
    settings_window.set_update_distribution_label(
        update_checker::distribution_label(update_display_distribution, lock_locale(&locale))
            .into(),
    );
    settings_window.set_update_check_label(lock_locale(&locale).settings().update_check.into());
    settings_window.set_update_release_label(lock_locale(&locale).settings().update_release.into());
    let labels = lock_locale(&locale).settings();
    settings_window.set_update_auto_label(labels.update_auto.into());
    settings_window.set_update_auto_description(labels.update_auto_description.into());
    settings_window.set_automatic_update_checks_enabled(
        lock_settings(&config).automatic_update_checks_enabled,
    );
    settings_window
        .set_update_check_available(update_display_distribution.can_offer_github_update());
    if update_display_distribution == Distribution::MicrosoftStore
        || update_display_distribution == Distribution::Unknown
    {
        settings_window.set_update_check_label(labels.update_store.into());
    }
    settings_window.on_open_update_release({
        let state = update_state.clone();
        move || {
            if qa_isolated {
                return;
            }
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
            if update_checks_allowed(qa_isolated, distribution) {
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
    if update_checks_allowed(qa_isolated, update_distribution) && !demo_mode {
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
    if !qa_isolated {
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
    }
    #[cfg(windows)]
    let worker_notifier = Some(native_tray.notifier());
    #[cfg(not(windows))]
    let worker_notifier = None;

    // The main and hover-preview windows reuse the same Slint component and
    // shared snapshot, but their lifecycle controllers own different HWNDs.
    #[cfg(windows)]
    let flyout = {
        let notifier = native_tray.notifier();
        flyout::Flyout::new(&window, move || notifier.icon_rect())
    };
    #[cfg(not(windows))]
    let flyout = flyout::Flyout::new(&window, || None);
    #[cfg(windows)]
    let hover_preview = {
        let notifier = native_tray.notifier();
        flyout::Flyout::new_with_main(&preview_window, Some(&window), move || notifier.icon_rect())
    };
    #[cfg(not(windows))]
    let hover_preview = flyout::Flyout::new_with_main(&preview_window, Some(&window), || None);
    window.on_close_flyout({
        let flyout = flyout.clone();
        move || flyout.close()
    });
    window.on_start_drag({
        let flyout = flyout.clone();
        move || flyout.start_drag()
    });
    window.on_refresh_finished({
        let hover_preview = hover_preview.clone();
        move || hover_preview.refresh_finished()
    });
    window.window().on_close_requested({
        let flyout = flyout.clone();
        move || {
            flyout.closed_externally();
            slint::CloseRequestResponse::HideWindow
        }
    });
    preview_window.window().on_close_requested({
        let hover_preview = hover_preview.clone();
        move || {
            hover_preview.close();
            slint::CloseRequestResponse::HideWindow
        }
    });
    {
        let flyout = flyout.clone();
        slint::Timer::single_shot(Duration::from_millis(0), move || flyout.warm_up());
    }
    {
        let hover_preview = hover_preview.clone();
        slint::Timer::single_shot(Duration::from_millis(0), move || hover_preview.warm_up());
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
        let hover_preview = hover_preview.clone();
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
                apply_latest_to_preview(&live_snapshot, &live_config, &live_locale);
                hover_preview.hover();
            }
            tray::TrayEvent::Pointer => hover_preview.pointer_over_icon(),
            tray::TrayEvent::OpenFull => {
                apply_latest_to_window(&live_window, &live_snapshot, &live_config, &live_locale);
                hover_preview.dismiss_preview();
                flyout.open_full();
            }
            tray::TrayEvent::Refresh => {
                apply_latest_to_window(&live_window, &live_snapshot, &live_config, &live_locale);
                hover_preview.menu_refresh(|| request_refresh(RefreshOrigin::Manual));
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
                hover_preview.dismiss_preview();
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
                apply_current_to_both(&window, &snapshot, &countdown_config, &countdown_locale);
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
    let theme_preview = preview_window.as_weak();
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
            if let Some(preview) = theme_preview.upgrade() {
                preview.set_accent_color(accent);
            }
            if let Some(panel) = theme_panel.upgrade() {
                panel.set_accent_color(accent);
            }
        },
    );

    #[cfg(windows)]
    let _popup_style_timer = {
        let popup = window.as_weak();
        let compact_popup = preview_window.as_weak();
        let flyout = flyout.clone();
        let hover_preview = hover_preview.clone();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(50),
            move || {
                let Some(popup) = popup.upgrade() else {
                    return;
                };
                let _ = apply_popup_taskbar_style(&popup);
                if let Some(compact_popup) = compact_popup.upgrade() {
                    let _ = apply_popup_taskbar_style(&compact_popup);
                }
                flyout.tick();
                hover_preview.tick();
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
            let hover_preview = hover_preview.clone();
            slint::Timer::single_shot(Duration::from_millis(300), move || {
                if preview_full {
                    flyout.open_full();
                } else {
                    hover_preview.preview_compact();
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
            preview_window
                .global::<Typography>()
                .set_family(family.clone());
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

    if qa_isolated {
        show_settings_centered(&settings_window);
    }

    // The flyout starts hidden: run the loop without showing any window
    // (ComponentHandle::run() would show the flyout).
    let settings_layout_timer = {
        let panel = settings_window.as_weak();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(500),
            move || {
                if let Some(panel) = panel.upgrade() {
                    if panel.window().is_visible() {
                        refresh_settings_layout_if_screen_changed(&panel);
                        #[cfg(all(windows, debug_assertions))]
                        if QA_GEOMETRY.load(Ordering::Acquire) {
                            log_settings_native_geometry(&panel);
                        }
                    }
                }
            },
        );
        timer
    };

    let result = slint::run_event_loop_until_quit();
    drop(settings_layout_timer);
    cancellation.cancel();
    if let Some(worker) = worker {
        let _ = worker.join();
    }
    result
}

/// Debug-only QA trace that separates the native HWND rectangles from Slint's
/// logical client/Flickable measurements. It runs only for an explicitly
/// isolated preview process.
#[cfg(all(windows, debug_assertions))]
fn log_settings_native_geometry(window: &SettingsWindow) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect};

    let handle = window.window().window_handle();
    let Ok(handle) = handle.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = handle.hwnd.get() as _;
    let mut client = RECT::default();
    let mut outer = RECT::default();
    let (client_ok, outer_ok) = unsafe {
        (
            GetClientRect(hwnd, &mut client) != 0,
            GetWindowRect(hwnd, &mut outer) != 0,
        )
    };
    let scale = window.window().scale_factor().max(0.5);
    let logical_window = window.window().size().to_logical(scale);
    eprintln!(
        "QA_GEOMETRY tab={} scale={scale:.4} slint_logical={:.2}x{:.2} native_client={}x{} client_delta_logical_h={:.2} native_outer={}x{} flickable_viewport={:.2} content={:.2} scroll_extent={:.2} content_y={:.2} required={} page_required={:.2} general={:.2} notifications={:.2} about_preferred={:.2} about_window_required={:.2} about_paddings={:.1}/{:.1} button_y={:.2} status_y={:.2} status_text_h={:.2} status_expansion={:.2} reclaimed={:.2} hero_h={:.2} features_y_h={:.2}/{:.2} feature_rows={:.2}/{:.2}/{:.2} connections_title_h={:.2} connections_y_h={:.2}/{:.2} connection_rows={:.2}/{:.2} distribution_h={:.2} links_h={:.2} legal_y_h={:.2}/{:.2} copyright_h={:.2} footer_bottom={:.2}",
        window.get_selected_tab(),
        logical_window.width,
        logical_window.height,
        if client_ok {
            client.right - client.left
        } else {
            -1
        },
        if client_ok {
            client.bottom - client.top
        } else {
            -1
        },
        if client_ok {
            (client.bottom - client.top) as f32 / scale - logical_window.height
        } else {
            f32::NAN
        },
        if outer_ok {
            outer.right - outer.left
        } else {
            -1
        },
        if outer_ok {
            outer.bottom - outer.top
        } else {
            -1
        },
        window.get_settings_scroll_viewport_height(),
        window.get_settings_scroll_content_height(),
        window.get_settings_scroll_max_extent(),
        window.get_settings_scroll_y(),
        window.get_required_settings_height(),
        window.get_active_page_required_height(),
        window.get_general_page_required_height(),
        window.get_notifications_page_required_height(),
        window.get_about_page_required_height(),
        window.get_about_page_window_required_height(),
        window.get_settings_about_top_padding(),
        window.get_settings_about_bottom_padding(),
        window.get_settings_about_update_actions_y(),
        window.get_settings_about_status_y(),
        window.get_settings_about_status_text_height(),
        window.get_settings_about_status_layout_expansion(),
        window.get_settings_about_status_space_reclaimed(),
        window.get_settings_about_hero_height(),
        window.get_settings_about_features_y(),
        window.get_settings_about_features_height(),
        window.get_settings_about_feature_tray_height(),
        window.get_settings_about_feature_local_height(),
        window.get_settings_about_feature_agents_height(),
        window.get_settings_about_connections_title_height(),
        window.get_settings_about_connections_y(),
        window.get_settings_about_connections_height(),
        window.get_settings_about_codex_row_height(),
        window.get_settings_about_claude_row_height(),
        window.get_settings_about_distribution_height(),
        window.get_settings_about_links_height(),
        window.get_settings_about_legal_note_y(),
        window.get_settings_about_legal_note_height(),
        window.get_settings_about_copyright_height(),
        window.get_settings_about_footer_bottom(),
    );
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
    fn acquire(qa_isolated: bool) -> Result<Option<Self>, ()> {
        use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
        use windows_sys::Win32::System::Threading::CreateMutexW;
        let name: Vec<u16> = if qa_isolated {
            "Local\\AgentUsageMonitor.Singleton.QA\0"
        } else {
            "Local\\AgentUsageMonitor.Singleton\0"
        }
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
/// is an explicit EXE Settings choice or the current-process CLI override,
/// and in the MSIX build the independent
/// `claude_bridge_listener_enabled` setting. Disabling Claude or the opt-in
/// stops the listener; neither ever turns the other on.
fn claude_bridge_wanted(bridge_argument: bool, settings: &AppSettings) -> bool {
    claude_bridge_wanted_for(packaged::is_packaged(), bridge_argument, settings)
}

fn claude_bridge_wanted_for(packaged: bool, bridge_argument: bool, settings: &AppSettings) -> bool {
    let opted_in = if packaged {
        settings.claude_bridge_listener_enabled
    } else {
        bridge_argument || settings.exe_claude_bridge_enabled.unwrap_or(false)
    };
    settings.claude_enabled && opted_in
}

fn exe_bridge_cli_override_active(bridge_argument: bool, settings: &AppSettings) -> bool {
    bridge_argument && settings.exe_claude_bridge_enabled != Some(true)
}

fn exe_bridge_toggle_state(bridge_argument: bool, settings: &AppSettings) -> (bool, bool) {
    (
        bridge_argument || settings.exe_claude_bridge_enabled.unwrap_or(false),
        exe_bridge_cli_override_active(bridge_argument, settings),
    )
}

/// `--enable-claude-bridge` was passed to this process.
fn claude_bridge_argument() -> bool {
    static ARGUMENT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ARGUMENT.get_or_init(|| std::env::args().any(|argument| argument == "--enable-claude-bridge"))
}

/// Flip the distribution-specific persisted listener opt-in. The CLI override
/// is session-only and is deliberately not persisted by the Settings control.
fn toggle_claude_bridge_listener(context: &CallbackContext, bridge_argument: bool) {
    let _mutation = settings_mutation_guard();
    if !packaged::is_packaged()
        && exe_bridge_cli_override_active(bridge_argument, &lock_settings(&context.0))
    {
        return;
    }
    let (config, path, _, _, _, _, snapshot) = context;
    let mut updated = lock_settings(config);
    if packaged::is_packaged() {
        updated.claude_bridge_listener_enabled = !updated.claude_bridge_listener_enabled;
    } else {
        updated.exe_claude_bridge_enabled =
            Some(!updated.exe_claude_bridge_enabled.unwrap_or(false));
    }
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
    if let Err(error) = autostart::set_enabled(requested, &executable) {
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

fn reconcile_autostart(config: &SharedSettings, path: &Path) {
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
    if let Err(error) = autostart::set_enabled(settings.launch_at_startup, &executable) {
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
        apply_current_to_both(&window, &snapshot, config, locale);
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
                        claude_refresher::ClaudeRefreshEvent::Failed { request_id, failure } => {
                            let updated = if let Ok(mut current) = claude_snapshot.lock() {
                                *current = match failure {
                                    claude_refresher::ClaudeRefreshFailure::CliMissing => {
                                        current.replace_claude_cli_missing()
                                    }
                                    failure => current.replace_claude_refresh_error(failure),
                                };
                                Some(current.clone())
                            } else { None };
                            eprintln!("CLAUDE_REFRESH: cache_updated request_id={} result=failed category={} retained_last_good=true", request_id.map_or_else(|| "background".into(), |id| id.to_string()), failure.category());
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
                *current = current.replace_claude_bridge_error();
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

fn apply_current_to_both(
    main: &MainWindow,
    snapshot: &adapter::UiSnapshot,
    config: &SharedSettings,
    locale: &SharedLocale,
) {
    apply_current(main, snapshot, config, locale);
    PREVIEW_WINDOW.with(|preview| {
        if let Some(preview) = preview.borrow().as_ref().and_then(slint::Weak::upgrade) {
            apply_current(&preview, snapshot, config, locale);
        }
    });
}

fn apply_latest_to_window(
    window: &slint::Weak<MainWindow>,
    snapshot: &SharedSnapshot,
    config: &SharedSettings,
    locale: &SharedLocale,
) {
    if let (Some(window), Ok(snapshot)) = (window.upgrade(), snapshot.lock()) {
        apply_current_to_both(&window, &snapshot, config, locale);
    }
}

fn apply_latest_to_preview(
    snapshot: &SharedSnapshot,
    config: &SharedSettings,
    locale: &SharedLocale,
) {
    if let Ok(snapshot) = snapshot.lock() {
        PREVIEW_WINDOW.with(|preview| {
            if let Some(preview) = preview.borrow().as_ref().and_then(slint::Weak::upgrade) {
                apply_current(&preview, &snapshot, config, locale);
            }
        });
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
        apply_current_to_both(&window, &snapshot, &config, &locale);
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
    let production_distribution = Distribution::current();
    #[cfg(debug_assertions)]
    let preview = if QA_ISOLATED.load(Ordering::Acquire) {
        QA_ABOUT_PREVIEW.lock().ok().and_then(|preview| *preview)
    } else {
        None
    };
    #[cfg(debug_assertions)]
    let distribution = about_display_distribution(
        production_distribution,
        QA_ISOLATED.load(Ordering::Acquire),
        preview,
    );
    #[cfg(not(debug_assertions))]
    let distribution = production_distribution;
    window.set_codex_enabled(settings.codex_enabled);
    window.set_claude_enabled(settings.claude_enabled);
    window.set_claude_cli_refresh_enabled(settings.claude_cli_refresh_enabled);
    // The QA preview substitutes About-only values. Preserve actual runtime
    // package identity for every Store-specific Settings control elsewhere.
    window.set_store_build(packaged::is_packaged());
    let (bridge_toggle_enabled, cli_override_active) = if packaged::is_packaged() {
        (settings.claude_bridge_listener_enabled, false)
    } else {
        exe_bridge_toggle_state(claude_bridge_argument(), settings)
    };
    window.set_claude_bridge_listener_enabled(bridge_toggle_enabled);
    window.set_claude_bridge_cli_override(cli_override_active);
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
    let startup_row = if QA_ISOLATED.load(Ordering::Acquire) {
        StartupRow {
            enabled: false,
            interactive: false,
            busy: false,
            text: StartupText::Normal,
        }
    } else if packaged::is_packaged() {
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
    let startup_row = if QA_ISOLATED.load(Ordering::Acquire) {
        startup_row
    } else {
        match PREVIEW_STARTUP.get().map(String::as_str) {
            Some("on") => StartupRow {
                enabled: true,
                interactive: true,
                busy: false,
                text: StartupText::Normal,
            },
            Some("error") => startup_row_error(),
            _ => startup_row,
        }
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
    window.set_title_label(if QA_ISOLATED.load(Ordering::Acquire) {
        format!("{} — QA", labels.title).into()
    } else {
        labels.title.into()
    });
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
    window.set_claude_bridge_description(
        if cli_override_active {
            labels.claude_bridge_cli_override
        } else {
            labels.claude_bridge_description
        }
        .into(),
    );
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
    let update_status = lock_update_status();
    configure_about_update_ui(window, distribution, locale, &update_status);
    #[cfg(debug_assertions)]
    if let Some(preview) = preview
        && preview.distribution.can_offer_github_update()
    {
        apply_about_preview_status(window, locale, preview.status);
    }
    window.set_update_auto_label(labels.update_auto.into());
    window.set_update_auto_description(labels.update_auto_description.into());
    window.set_automatic_update_checks_enabled(settings.automatic_update_checks_enabled);
    window.set_update_release_label(labels.update_release.into());
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

fn configure_about_update_ui(
    window: &SettingsWindow,
    distribution: Distribution,
    locale: Locale,
    update_status: &UpdateStatus,
) {
    let labels = locale.settings();
    window.set_update_check_label(if distribution == Distribution::MicrosoftStore {
        labels.update_store.into()
    } else {
        labels.update_check.into()
    });
    window.set_update_distribution_label(
        update_checker::distribution_label(distribution, locale).into(),
    );
    window.set_update_check_available(distribution.can_offer_github_update());
    window.set_update_checking(matches!(update_status, UpdateStatus::Checking));

    if distribution == Distribution::MicrosoftStore {
        window.set_update_status(labels.update_store.into());
        window.set_update_available(false);
        window.set_update_release_visible(false);
        window.set_update_checking(false);
        return;
    }

    if !distribution.can_offer_github_update() {
        window.set_update_status(labels.update_unknown.into());
        window.set_update_available(false);
        window.set_update_release_visible(false);
        window.set_update_checking(false);
        return;
    }

    set_update_status_text(window, labels, update_status);
}

#[cfg(any(debug_assertions, test))]
fn apply_about_preview_status(
    window: &SettingsWindow,
    locale: Locale,
    preview_status: AboutPreviewStatus,
) {
    let labels = locale.settings();
    let (status, available, release_visible, checking) = match preview_status {
        AboutPreviewStatus::Idle => (String::new(), false, false, false),
        AboutPreviewStatus::Checking => (labels.update_checking.into(), false, false, true),
        AboutPreviewStatus::Available => (
            format!(
                "{}: v{} → v{}",
                labels.update_available,
                env!("CARGO_PKG_VERSION"),
                env!("CARGO_PKG_VERSION")
            ),
            true,
            true,
            false,
        ),
        AboutPreviewStatus::Long => (
            format!(
                "{}: v{} → v{} · {}",
                labels.update_available,
                env!("CARGO_PKG_VERSION"),
                env!("CARGO_PKG_VERSION"),
                [labels.update_available; 5].join(" · ")
            ),
            true,
            true,
            false,
        ),
        AboutPreviewStatus::Current => (labels.update_current.into(), false, false, false),
        AboutPreviewStatus::Offline => (labels.update_offline.into(), false, false, false),
        AboutPreviewStatus::Http => (format!("{} (503)", labels.update_http), false, false, false),
        AboutPreviewStatus::Invalid => (labels.update_invalid.into(), false, false, false),
        AboutPreviewStatus::Timeout => (labels.update_timeout.into(), false, false, false),
    };
    window.set_update_status(status.into());
    window.set_update_available(available);
    window.set_update_release_visible(release_visible);
    window.set_update_checking(checking);
}

fn request_update_check(
    panel: slint::Weak<SettingsWindow>,
    state: Arc<Mutex<UiState>>,
    in_flight: Arc<AtomicBool>,
    distribution: Distribution,
    locale: SharedLocale,
    persistence: Option<(SharedSettings, PathBuf, watch::Sender<AppSettings>)>,
) {
    if !update_checks_allowed(QA_ISOLATED.load(Ordering::Acquire), distribution)
        || in_flight.swap(true, Ordering::AcqRel)
    {
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
/// then nothing is offered for copying. `store_build` picks the runtime's
/// distribution context; both distributions use an explicit Settings opt-in,
/// while EXE also retains the current-process `--enable-claude-bridge` flag.
/// Another tool's status line is
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
    if window.window().is_visible() {
        activate_window(window.window());
        return;
    }
    if SETTINGS_OPEN_PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    refresh_statusline_state();
    apply_statusline_row(window);
    // MSIX: the user may have changed the startup task in Windows meanwhile;
    // re-read it off the UI thread (the toggle waits until the result is in).
    if packaged::is_packaged()
        && autostart::packaged_controller().submit(autostart::StartupRequest::Refresh)
    {
        window.set_startup_busy(true);
    }
    // Give Slint one event-loop turn to lay out wrapped text at the intended
    // width before measuring. The hidden window gets its final size before it
    // is shown, avoiding a full-work-area first-open measurement and resize.
    prepare_settings_content_measurement(window);
    let weak = window.as_weak();
    slint::Timer::single_shot(Duration::from_millis(16), move || {
        if let Some(window) = weak.upgrade() {
            show_settings_measured(&window);
        }
        SETTINGS_OPEN_PENDING.store(false, Ordering::Release);
    });
}

fn show_settings_measured(window: &SettingsWindow) {
    #[cfg(windows)]
    let pre_fit_logical_size = window
        .window()
        .size()
        .to_logical(window.window().scale_factor().max(0.5));
    let app_requested_size = recalculate_settings_layout(window, true, true);
    #[cfg(not(windows))]
    let _ = app_requested_size;
    #[cfg(windows)]
    let opened_with_automatic_size = retained_settings_window_size().is_none();
    let _ = window.show();
    #[cfg(windows)]
    if !install_settings_size_tracker(window.window()) {
        retry_settings_size_tracker(window.as_weak(), 20);
    }
    activate_window(window.window());
    let mode = match window.get_theme_kind() {
        1 => ThemeMode::BluishDark,
        2 => ThemeMode::Light,
        _ => ThemeMode::Graphite,
    };
    if !theme::apply_title_bar(window.window(), mode) {
        retry_title_bar(window.as_weak(), mode, 20);
    }
    // Native text metrics can settle only after the window is shown. On every
    // open with an automatic size (no user-selected size retained), remeasure
    // once and apply the common page height if the user has not resized the
    // window meanwhile. A user-selected size remains authoritative.
    //
    // On Windows, Winit applies `set_size` asynchronously: `Window::size()`
    // keeps the old value until `WindowEvent::Resized` arrives. A size read
    // right after the first fit is therefore stale, and comparing against it
    // misclassifies the app's own resize as a manual one. Compare against the
    // sizes the app itself owns instead: the size it requested and the pre-fit
    // size (in case the native resize has not landed yet).
    #[cfg(windows)]
    let weak = window.as_weak();
    #[cfg(windows)]
    slint::Timer::single_shot(Duration::from_millis(150), move || {
        if let Some(window) = weak.upgrade() {
            let scale = window.window().scale_factor().max(0.5);
            let current = window.window().size().to_logical(scale);
            let retained_size = retained_settings_window_size();
            let resizing = settings_user_is_dragging_or_resizing();
            let visible = window.window().is_visible();
            let app_owned_sizes = native_first_show_app_owned_sizes(
                (pre_fit_logical_size.width, pre_fit_logical_size.height),
                app_requested_size,
            );
            let fit_allowed = native_first_show_remeasure_allowed(
                opened_with_automatic_size && retained_size.is_none() && visible,
                &app_owned_sizes,
                (current.width, current.height),
                resizing,
            );
            #[cfg(debug_assertions)]
            if QA_GEOMETRY.load(Ordering::Acquire) {
                eprintln!(
                    "QA_GEOMETRY_POST_SHOW visible={visible} auto_size={} retained={retained_size:?} resizing={resizing} pre_fit={:.2}x{:.2} app_requested={app_requested_size:?} current={:.2}x{:.2} scale={scale:.4} fit_allowed={fit_allowed}",
                    opened_with_automatic_size,
                    pre_fit_logical_size.width,
                    pre_fit_logical_size.height,
                    current.width,
                    current.height,
                );
            }
            if fit_allowed {
                #[cfg(debug_assertions)]
                if QA_GEOMETRY.load(Ordering::Acquire) {
                    eprintln!("QA_GEOMETRY_PHASE=post_show_before_native_fit");
                    log_settings_native_geometry(&window);
                }
                let _ = recalculate_settings_layout(&window, true, false);
                #[cfg(debug_assertions)]
                if QA_GEOMETRY.load(Ordering::Acquire) {
                    eprintln!("QA_GEOMETRY_PHASE=post_show_after_native_fit");
                    log_settings_native_geometry(&window);
                }
            } else {
                let _ = recalculate_settings_layout(&window, false, true);
            }
        }
    });
}

/// Sizes the app itself left or set on the Settings window during the first
/// fit: the pre-fit size (the native resize may still be pending) and the
/// clamped size the first fit requested.
#[cfg(any(windows, test))]
fn native_first_show_app_owned_sizes(
    pre_fit_size: (f32, f32),
    app_requested_size: Option<(f32, f32)>,
) -> Vec<(f32, f32)> {
    std::iter::once(pre_fit_size)
        .chain(app_requested_size)
        .collect()
}

/// A first-show remeasure may resize the window only while its size is still
/// one the app owns; any other size means the user resized it.
#[cfg(any(windows, test))]
fn native_first_show_remeasure_allowed(
    opened_with_automatic_size: bool,
    app_owned_sizes: &[(f32, f32)],
    current_size: (f32, f32),
    user_is_resizing: bool,
) -> bool {
    opened_with_automatic_size
        && !user_is_resizing
        && app_owned_sizes.iter().any(|size| {
            (current_size.0 - size.0).abs() <= 0.5 && (current_size.1 - size.1).abs() <= 0.5
        })
}

#[cfg(windows)]
const SETTINGS_SIZE_TRACKER_ID: usize = 0x4155_4d53; // "AUMS"

/// Subclasses the Settings HWND to observe the native size loop. Installing
/// the same proc/id again only updates it, so calling this on every show is
/// safe. Returns false while the native window does not exist yet.
#[cfg(windows)]
fn install_settings_size_tracker(window: &slint::Window) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::UI::Shell::SetWindowSubclass;
    let handle = window.window_handle();
    let Ok(handle) = handle.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return false;
    };
    unsafe {
        SetWindowSubclass(
            handle.hwnd.get() as _,
            Some(settings_size_tracker_proc),
            SETTINGS_SIZE_TRACKER_ID,
            0,
        ) != 0
    }
}

#[cfg(windows)]
fn retry_settings_size_tracker(window: slint::Weak<SettingsWindow>, attempts: u8) {
    if attempts == 0 {
        return;
    }
    slint::Timer::single_shot(Duration::from_millis(50), move || {
        if let Some(panel) = window.upgrade()
            && !install_settings_size_tracker(panel.window())
        {
            retry_settings_size_tracker(panel.as_weak(), attempts - 1);
        }
    });
}

/// Records only the size-loop facts; it never calls back into Slint or
/// resizes the window, so it cannot start a resize loop. `try_lock` keeps a
/// message dispatched while the layout lock is held from deadlocking.
#[cfg(windows)]
unsafe extern "system" fn settings_size_tracker_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_NCDESTROY, WM_SIZING,
    };
    match message {
        WM_ENTERSIZEMOVE => {
            if let Ok(mut layout) = SETTINGS_LAYOUT.try_lock() {
                layout.size_ownership.enter_size_move();
            }
        }
        WM_SIZING => {
            if let Ok(mut layout) = SETTINGS_LAYOUT.try_lock() {
                layout.size_ownership.user_sizing();
            }
        }
        WM_EXITSIZEMOVE => {
            let mut client = RECT::default();
            let dpi = unsafe { GetDpiForWindow(hwnd) };
            let scale = if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 };
            let logical = if unsafe { GetClientRect(hwnd, &mut client) } != 0 {
                (
                    (client.right - client.left) as f32 / scale,
                    (client.bottom - client.top) as f32 / scale,
                )
            } else {
                (0.0, 0.0)
            };
            if let Ok(mut layout) = SETTINGS_LAYOUT.try_lock() {
                let _recorded = layout.size_ownership.exit_size_move(logical);
                #[cfg(debug_assertions)]
                if QA_GEOMETRY.load(Ordering::Acquire) {
                    eprintln!(
                        "QA_GEOMETRY_SIZE_LOOP_END client_logical={:.2}x{:.2} user_size_recorded={_recorded:?}",
                        logical.0, logical.1
                    );
                }
            }
        }
        WM_NCDESTROY => unsafe {
            RemoveWindowSubclass(
                hwnd,
                Some(settings_size_tracker_proc),
                SETTINGS_SIZE_TRACKER_ID,
            );
        },
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
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

fn settings_geometry(
    requested_width: f32,
    requested_height: f32,
    screen: SettingsScreenSignature,
) -> SettingsGeometry {
    let scale = f32::from_bits(screen.scale_bits).max(0.5);
    let margin = (SETTINGS_MARGIN_LOGICAL * scale).round() as i32;
    let caption = (SETTINGS_CAPTION_LOGICAL * scale).round() as i32;
    let work_width = (screen.work_area.right - screen.work_area.left).max(1);
    let work_height = (screen.work_area.bottom - screen.work_area.top).max(1);
    let max_width = (work_width - 2 * margin).max(1) as f32 / scale;
    let max_height = (work_height - 2 * margin - caption).max(1) as f32 / scale;
    let width = requested_width.min(max_width).max(1.0);
    let height = requested_height.min(max_height).max(1.0);
    let minimum_height = SETTINGS_MIN_HEIGHT_LOGICAL.min(height);
    let physical_width = (width * scale).round() as i32;
    let physical_height = (height * scale).round() as i32;
    let x = screen.work_area.left + ((work_width - physical_width).max(0) / 2);
    let y = screen.work_area.top + ((work_height - physical_height - caption).max(0) / 2);
    SettingsGeometry {
        width,
        height,
        minimum_height,
        x,
        y,
    }
}

fn prepare_settings_content_measurement(window: &SettingsWindow) {
    let scale = window.window().scale_factor().max(0.5);
    let current = window.window().size().to_logical(scale);
    let retained_size = retained_settings_window_size();
    let (width, retained_height) = settings_open_size(
        window.get_default_width(),
        window.get_settings_window_height(),
        retained_size,
    );
    let height = retained_height.max(current.height).max(1.0);

    // A newly created hidden Winit window can still have a zero-sized layout.
    // Set its intended width while hidden so wrapped text is measured at the
    // real content width, not at zero (which can inflate preferred heights).
    if (current.width - width).abs() > 0.5 || current.height <= 0.0 {
        window
            .window()
            .set_size(slint::LogicalSize::new(width, height));
    }
}

/// Applies the Settings window geometry and returns the logical size it
/// requested (after work-area/DPI clamping), if any. That size is the target
/// even when the native resize lands later.
fn recalculate_settings_layout(
    window: &SettingsWindow,
    use_common_size: bool,
    center: bool,
) -> Option<(f32, f32)> {
    if use_common_size {
        prepare_settings_content_measurement(window);
    }
    let required_height = window.get_required_settings_height();
    let (common_width, common_height) = settings_open_size(
        window.get_default_width(),
        required_height,
        retained_settings_window_size(),
    );
    #[cfg(windows)]
    let monitor = settings_fit_monitor(window.window().is_visible());
    #[cfg(windows)]
    let screen = settings_screen_signature(window, monitor == SettingsMonitor::Cursor);
    #[cfg(all(windows, debug_assertions))]
    if QA_GEOMETRY.load(Ordering::Acquire) && screen.is_none() {
        eprintln!(
            "QA_GEOMETRY_NATIVE_FIT_SKIPPED reason=screen_lookup_failed common={use_common_size} monitor={monitor:?} required={required_height:.2}"
        );
    }
    #[cfg(windows)]
    if let Some(screen) = screen {
        let (requested_width, requested_height) = if use_common_size {
            (common_width, common_height)
        } else {
            let scale = f32::from_bits(screen.scale_bits);
            let current = window.window().size().to_logical(scale);
            (current.width, current.height)
        };
        let geometry = settings_geometry(requested_width, requested_height, screen);
        window.set_settings_window_height(geometry.height);
        window.set_minimum_settings_window_height(geometry.minimum_height);

        let logical_size = slint::LogicalSize::new(geometry.width, geometry.height);
        let current = window
            .window()
            .size()
            .to_logical(f32::from_bits(screen.scale_bits));
        let will_resize = (current.width - geometry.width).abs() > 0.5
            || (current.height - geometry.height).abs() > 0.5;
        #[cfg(debug_assertions)]
        if QA_GEOMETRY.load(Ordering::Acquire) {
            eprintln!(
                "QA_GEOMETRY_NATIVE_FIT monitor={monitor:?} required={required_height:.2} requested={requested_width:.2}x{requested_height:.2} work_area={}x{}@{},{} scale={:.4} clamped={:.2}x{:.2} current={:.2}x{:.2} set_size={will_resize}",
                screen.work_area.right - screen.work_area.left,
                screen.work_area.bottom - screen.work_area.top,
                screen.work_area.left,
                screen.work_area.top,
                f32::from_bits(screen.scale_bits),
                geometry.width,
                geometry.height,
                current.width,
                current.height,
            );
        }
        if will_resize {
            window.window().set_size(logical_size);
        }
        #[cfg(debug_assertions)]
        if QA_GEOMETRY.load(Ordering::Acquire) {
            let after = window
                .window()
                .size()
                .to_logical(f32::from_bits(screen.scale_bits));
            eprintln!(
                "QA_GEOMETRY_NATIVE_FIT_RESULT requested={:.2}x{:.2} immediate={:.2}x{:.2}",
                geometry.width, geometry.height, after.width, after.height,
            );
        }
        let position = if center {
            slint::PhysicalPosition::new(geometry.x, geometry.y)
        } else {
            clamp_settings_position(window, screen, geometry)
        };
        if window.window().position() != position {
            window.window().set_position(position);
        }
        if let Ok(mut layout) = SETTINGS_LAYOUT.lock() {
            layout.screen = Some(screen);
        }
        return Some((geometry.width, geometry.height));
    }
    #[cfg(not(windows))]
    if use_common_size {
        window.set_settings_window_height(common_height);
        window
            .window()
            .set_size(slint::LogicalSize::new(common_width, common_height));
        return Some((common_width, common_height));
    }
    None
}

fn settings_open_size(
    default_width: f32,
    required_height: f32,
    retained_size: Option<(f32, f32)>,
) -> (f32, f32) {
    retained_size.unwrap_or((default_width, required_height))
}

fn retained_settings_window_size() -> Option<(f32, f32)> {
    SETTINGS_LAYOUT
        .lock()
        .ok()
        .and_then(|layout| layout.size_ownership.user_size())
}

#[cfg(windows)]
fn clamp_settings_position(
    window: &SettingsWindow,
    screen: SettingsScreenSignature,
    geometry: SettingsGeometry,
) -> slint::PhysicalPosition {
    let scale = f32::from_bits(screen.scale_bits);
    let width = (geometry.width * scale).round() as i32;
    let height = (geometry.height * scale).round() as i32;
    let caption = (SETTINGS_CAPTION_LOGICAL * scale).round() as i32;
    let left = screen.work_area.left + (SETTINGS_MARGIN_LOGICAL * scale).round() as i32;
    let top = screen.work_area.top + (SETTINGS_MARGIN_LOGICAL * scale).round() as i32;
    let max_x = (screen.work_area.right - width - (SETTINGS_MARGIN_LOGICAL * scale).round() as i32)
        .max(left);
    let max_y = (screen.work_area.bottom
        - height
        - caption
        - (SETTINGS_MARGIN_LOGICAL * scale).round() as i32)
        .max(top);
    let current = window.window().position();
    slint::PhysicalPosition::new(current.x.clamp(left, max_x), current.y.clamp(top, max_y))
}

#[cfg(windows)]
fn settings_screen_signature(
    window: &SettingsWindow,
    use_cursor: bool,
) -> Option<SettingsScreenSignature> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, MonitorFromWindow,
    };
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let monitor = unsafe {
        if use_cursor {
            let mut cursor = POINT { x: 0, y: 0 };
            (GetCursorPos(&mut cursor) != 0)
                .then(|| MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST))?
        } else {
            let handle = window.window().window_handle();
            let Ok(handle) = handle.window_handle() else {
                return None;
            };
            let RawWindowHandle::Win32(handle) = handle.as_raw() else {
                return None;
            };
            MonitorFromWindow(handle.hwnd.get() as _, MONITOR_DEFAULTTONEAREST)
        }
    };
    if monitor.is_null() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        rcMonitor: RECT::default(),
        rcWork: RECT::default(),
        dwFlags: 0,
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return None;
    }
    let mut dpi_x = 96;
    let mut dpi_y = 96;
    let hr = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
    let scale = if hr >= 0 {
        dpi_x as f32 / 96.0
    } else {
        window.window().scale_factor()
    };
    Some(SettingsScreenSignature {
        monitor: monitor as usize,
        work_area: PhysicalWorkArea {
            left: info.rcWork.left,
            top: info.rcWork.top,
            right: info.rcWork.right,
            bottom: info.rcWork.bottom,
        },
        scale_bits: scale.to_bits(),
    })
}

#[cfg(windows)]
fn refresh_settings_layout_if_screen_changed(window: &SettingsWindow) {
    // If the user is dragging or resizing the native window between monitors,
    // defer bounds adjustment until release so the timer cannot fight them.
    if settings_user_is_dragging_or_resizing() {
        return;
    }
    if let Some(current) = settings_screen_signature(window, false) {
        let previous = SETTINGS_LAYOUT.lock().ok().and_then(|layout| layout.screen);
        if previous != Some(current) {
            let _ = recalculate_settings_layout(window, false, false);
        }
    }
}

#[cfg(windows)]
fn settings_user_is_dragging_or_resizing() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    unsafe { GetAsyncKeyState(VK_LBUTTON as i32) < 0 }
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
    use super::{
        AboutPreview, AboutPreviewStatus, Distribution, PhysicalWorkArea, SettingsScreenSignature,
        UpdateStatus, about_display_distribution, apply_about_preview_status,
        configure_about_update_ui, settings_geometry, update_checks_allowed,
    };

    #[test]
    fn about_distribution_preview_only_overrides_isolated_display_state() {
        let production = Distribution::GitHubPortable;
        let store_preview = AboutPreview {
            distribution: Distribution::MicrosoftStore,
            status: AboutPreviewStatus::Idle,
        };

        assert_eq!(
            about_display_distribution(production, true, Some(store_preview)),
            Distribution::MicrosoftStore
        );
        assert_eq!(
            about_display_distribution(production, true, None),
            production
        );
        assert_eq!(
            about_display_distribution(production, false, Some(store_preview)),
            production
        );
    }

    #[test]
    fn isolated_qa_blocks_update_network_actions() {
        assert!(!update_checks_allowed(true, Distribution::GitHubPortable));
        assert!(update_checks_allowed(false, Distribution::GitHubPortable));
        assert!(!update_checks_allowed(false, Distribution::MicrosoftStore));

        let preview = AboutPreview::from_args(&[
            "aum".into(),
            "--qa-about-distribution=portable".into(),
            "--qa-about-status=idle".into(),
        ])
        .unwrap();
        assert!(matches!(preview.status, AboutPreviewStatus::Idle));
    }

    #[test]
    fn native_first_show_remeasure_respects_retained_and_user_resized_sizes() {
        let owned = native_first_show_app_owned_sizes((720.0, 738.0), Some((720.0, 738.0)));
        assert!(native_first_show_remeasure_allowed(
            true,
            &owned,
            (720.0, 738.0),
            false,
        ));
        // A retained/manual size from an earlier open stays authoritative.
        assert!(!native_first_show_remeasure_allowed(
            false,
            &owned,
            (720.0, 738.0),
            false,
        ));
        // A size the app never requested is a manual resize.
        assert!(!native_first_show_remeasure_allowed(
            true,
            &owned,
            (720.0, 748.0),
            false,
        ));
        assert!(!native_first_show_remeasure_allowed(
            true,
            &owned,
            (880.0, 690.0),
            false,
        ));
        // An active drag/resize always blocks the fit.
        assert!(!native_first_show_remeasure_allowed(
            true,
            &owned,
            (720.0, 738.0),
            true,
        ));
    }

    /// Regression for the native Windows QA trace on PR #14: the first fit
    /// requested 720x738 while Winit still reported 720x480 (its `set_size` is
    /// asynchronous), the resize landed before the post-show check, and the
    /// app's own resize was misread as a manual one (`fit_allowed=false`).
    #[test]
    fn native_first_show_fit_is_allowed_after_asynchronous_app_resize() {
        let stale_pre_fit = (720.0, 480.0);
        let owned = native_first_show_app_owned_sizes(stale_pre_fit, Some((720.0, 738.0)));

        // The app's own resize landed before the post-show check.
        assert!(native_first_show_remeasure_allowed(
            true,
            &owned,
            (720.0, 738.0),
            false,
        ));
        // The native resize has not landed yet.
        assert!(native_first_show_remeasure_allowed(
            true,
            &owned,
            stale_pre_fit,
            false,
        ));
        // The previous check (current vs. the stale snapshot only) rejected it.
        assert!(!native_first_show_remeasure_allowed(
            true,
            &[stale_pre_fit],
            (720.0, 738.0),
            false,
        ));
        // A user resize to any other size is still respected.
        assert!(!native_first_show_remeasure_allowed(
            true,
            &owned,
            (760.0, 700.0),
            false,
        ));
    }

    /// After the fit is allowed, the second pass requests the remeasured
    /// height (748), not the current client height (738), while the
    /// work-area/DPI clamp still bounds it on small screens.
    #[test]
    fn native_second_fit_requests_remeasured_height_within_work_area() {
        let roomy = SettingsScreenSignature {
            work_area: PhysicalWorkArea {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1032,
            },
            ..screen(1.0)
        };
        let (width, height) = settings_open_size(720.0, 748.0, None);
        let fitted = settings_geometry(width, height, roomy);
        assert_eq!((fitted.width, fitted.height), (720.0, 748.0));

        // Small work area: the clamp wins and the Flickable scrolls the rest.
        let small = SettingsScreenSignature {
            work_area: PhysicalWorkArea {
                left: 0,
                top: 0,
                right: 1366,
                bottom: 728,
            },
            ..screen(1.0)
        };
        let clamped = settings_geometry(width, height, small);
        assert!(clamped.height < 748.0);

        // 150 % DPI: logical request stays 748 when the work area allows it.
        let hidpi = SettingsScreenSignature {
            work_area: PhysicalWorkArea {
                left: 0,
                top: 0,
                right: 2560,
                bottom: 1392,
            },
            ..screen(1.5)
        };
        let scaled = settings_geometry(width, height, hidpi);
        assert_eq!((scaled.width, scaled.height), (720.0, 748.0));
    }

    #[test]
    fn native_fit_resolves_748_required_height_from_738_client() {
        let screen = SettingsScreenSignature {
            work_area: PhysicalWorkArea {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1032,
            },
            ..screen(1.0)
        };
        let initial: (f32, f32) = (720.0, 738.0);
        let required_about: f32 = 748.0;
        let initial_scroll_extent = (required_about - initial.1).max(0.0);
        assert_eq!(initial_scroll_extent, 10.0);
        let owned = native_first_show_app_owned_sizes((720.0, 480.0), Some(initial));
        assert!(native_first_show_remeasure_allowed(
            true, &owned, initial, false,
        ));

        let fitted = settings_geometry(720.0, required_about, screen);
        assert_eq!((fitted.width, fitted.height), (720.0, 748.0));
        assert_eq!((required_about - fitted.height).max(0.0), 0.0);
    }

    #[test]
    fn normal_settings_content_fits_below_a_1080p_work_area() {
        i_slint_backend_testing::init_no_event_loop();
        for locale in [Locale::Uk, Locale::En, Locale::Pl, Locale::Ru] {
            let window = SettingsWindow::new().unwrap();
            window
                .window()
                .set_size(slint::LogicalSize::new(0.0, 480.0));
            update_settings_window(
                &window,
                &AppSettings::default(),
                locale,
                false,
                &adapter::UiSnapshot::unavailable(),
                now_unix(),
            );

            let zero_width_required = window.get_required_settings_height();
            prepare_settings_content_measurement(&window);
            let measurement_width = window.get_settings_content_width();
            let general = window.get_general_page_required_height();
            let notifications = window.get_notifications_page_required_height();
            let about = window.get_about_page_required_height();
            let required = window.get_required_settings_height();
            let work_area = SettingsScreenSignature {
                work_area: PhysicalWorkArea {
                    left: 0,
                    top: 0,
                    right: 1920,
                    bottom: 1032,
                },
                ..screen(1.0)
            };
            let geometry = settings_geometry(720.0, required, work_area);

            println!(
                "{locale:?}: zero-width max={zero_width_required:.1}px; width={measurement_width:.1}px; General={general:.1}px Notifications={notifications:.1}px About={about:.1}px max={required:.1}px"
            );
            assert!(measurement_width > 500.0);
            assert!(zero_width_required > 1032.0);
            assert!(zero_width_required >= required);
            assert_eq!(required, general.max(notifications).max(about));
            assert_eq!(
                geometry.height, required,
                "work area is only an upper bound"
            );
            assert!(
                geometry.height < 0.8 * 1032.0,
                "normal {locale:?} content should not nearly fill the 1080p work area: {geometry:?}"
            );
        }
    }

    #[test]
    fn second_common_measurement_fits_content_that_grows_after_initial_layout() {
        i_slint_backend_testing::init_no_event_loop();
        let window = SettingsWindow::new().unwrap();
        update_settings_window(
            &window,
            &AppSettings::default(),
            Locale::En,
            false,
            &adapter::UiSnapshot::unavailable(),
            now_unix(),
        );
        prepare_settings_content_measurement(&window);
        let first_measurement = window.get_required_settings_height();
        window.set_settings_window_height(first_measurement);
        window
            .window()
            .set_size(slint::LogicalSize::new(720.0, first_measurement));

        // Model text metrics settling after the first measurement: the About
        // legal note wraps to additional lines only after its final text lands.
        window.set_legal_note("native text reflow ".repeat(18).into());
        let settled_required = window.get_required_settings_height();
        assert!(settled_required > first_measurement);
        let (width, common_height) = settings_open_size(720.0, settled_required, None);
        window.set_settings_window_height(common_height);
        window
            .window()
            .set_size(slint::LogicalSize::new(width, common_height));
        let final_size = window.window().size();

        for tab in 0..3 {
            window.invoke_select_settings_tab(tab);
            assert_eq!(window.window().size(), final_size);
            assert_eq!(window.get_settings_scroll_max_extent(), 0.0);
            window.invoke_scroll_settings_to_bottom();
            assert_eq!(window.get_settings_scroll_y(), 0.0);
        }
    }

    #[test]
    fn about_distribution_and_status_layout_is_shared_and_stable() {
        i_slint_backend_testing::init_no_event_loop();
        let locales = [Locale::Uk, Locale::En, Locale::Pl, Locale::Ru];
        let distributions = [
            Distribution::GitHubPortable,
            Distribution::GitHubInstaller,
            Distribution::MicrosoftStore,
        ];
        let statuses = [
            AboutPreviewStatus::Idle,
            AboutPreviewStatus::Checking,
            AboutPreviewStatus::Current,
            AboutPreviewStatus::Idle,
            AboutPreviewStatus::Checking,
            AboutPreviewStatus::Available,
            AboutPreviewStatus::Idle,
            AboutPreviewStatus::Checking,
            AboutPreviewStatus::Long,
            AboutPreviewStatus::Idle,
            AboutPreviewStatus::Checking,
            AboutPreviewStatus::Offline,
            AboutPreviewStatus::Idle,
            AboutPreviewStatus::Checking,
            AboutPreviewStatus::Http,
            AboutPreviewStatus::Idle,
            AboutPreviewStatus::Checking,
            AboutPreviewStatus::Invalid,
            AboutPreviewStatus::Idle,
            AboutPreviewStatus::Checking,
            AboutPreviewStatus::Timeout,
        ];

        for locale in locales {
            let labels = locale.settings();
            for distribution in distributions {
                let window = SettingsWindow::new().unwrap();
                update_settings_window(
                    &window,
                    &AppSettings::default(),
                    locale,
                    false,
                    &adapter::UiSnapshot::unavailable(),
                    now_unix(),
                );
                configure_about_update_ui(&window, distribution, locale, &UpdateStatus::Idle);
                window.set_selected_tab(2);
                let store = distribution == Distribution::MicrosoftStore;
                let mut expected_window_size = None;
                let mut measurements = Vec::new();
                let mut expected_button_gap = None;
                let mut expected_required_height = None;
                let mut expected_button_y: Option<f32> = None;
                let mut expected_status_y: Option<f32> = None;
                let mut expected_footer_bottom: Option<f32> = None;

                for status in statuses {
                    configure_about_update_ui(&window, distribution, locale, &UpdateStatus::Idle);
                    if !store {
                        apply_about_preview_status(&window, locale, status);
                    }
                    if store {
                        assert!(!window.get_update_check_available());
                    } else {
                        let gap = window.get_about_update_button_gap();
                        assert!(
                            (gap - 12.0).abs() < 0.1,
                            "idle spacing from the distribution label to the update button should remain 12 logical px for {locale:?}/{distribution:?}/{status:?}, got {gap:.2}"
                        );
                        if let Some(expected) = expected_button_gap {
                            assert_eq!(gap, expected, "button position changed across statuses");
                        } else {
                            expected_button_gap = Some(gap);
                        }
                    }
                    assert_eq!(
                        window.get_update_check_available(),
                        !store,
                        "GitHub check visibility for {distribution:?}/{locale:?}"
                    );
                    assert_eq!(
                        window.get_update_distribution_label().as_str(),
                        super::update_checker::distribution_label(distribution, locale),
                        "localized distribution label for {distribution:?}/{locale:?}"
                    );
                    assert_eq!(
                        window.get_update_release_visible(),
                        !store
                            && matches!(
                                status,
                                AboutPreviewStatus::Available | AboutPreviewStatus::Long
                            ),
                        "release action visibility for {distribution:?}/{locale:?}/{status:?}"
                    );
                    assert_eq!(
                        window.get_update_available(),
                        !store
                            && matches!(
                                status,
                                AboutPreviewStatus::Available | AboutPreviewStatus::Long
                            )
                    );
                    if store {
                        assert_eq!(window.get_update_status().as_str(), labels.update_store);
                    } else if matches!(status, AboutPreviewStatus::Idle) {
                        assert!(window.get_update_status().is_empty());
                        assert_eq!(
                            window.get_app_version().as_str(),
                            format!("v{}", env!("CARGO_PKG_VERSION"))
                        );
                    } else if matches!(status, AboutPreviewStatus::Checking) {
                        assert_eq!(window.get_update_status().as_str(), labels.update_checking);
                        assert!(window.get_update_checking());
                    } else if matches!(status, AboutPreviewStatus::Current) {
                        assert_eq!(window.get_update_status().as_str(), labels.update_current);
                    } else if matches!(status, AboutPreviewStatus::Offline) {
                        assert_eq!(window.get_update_status().as_str(), labels.update_offline);
                    } else if matches!(status, AboutPreviewStatus::Http) {
                        assert!(
                            window
                                .get_update_status()
                                .as_str()
                                .starts_with(labels.update_http)
                        );
                    } else if matches!(status, AboutPreviewStatus::Invalid) {
                        assert_eq!(window.get_update_status().as_str(), labels.update_invalid);
                    } else if matches!(status, AboutPreviewStatus::Timeout) {
                        assert_eq!(window.get_update_status().as_str(), labels.update_timeout);
                    }
                    if !store && matches!(status, AboutPreviewStatus::Long) {
                        assert!(window.get_update_status().len() > 100);
                    }

                    let required = window.get_required_settings_height();
                    let about_height = window.get_about_page_required_height();
                    measurements.push((status, about_height, required));
                    if let Some(expected) = expected_required_height {
                        assert_eq!(
                            required, expected,
                            "window sizing basis changed across status transitions for {distribution:?}/{locale:?}/{status:?}"
                        );
                    } else {
                        expected_required_height = Some(required);
                    }
                    let status_height = window.get_update_status_slot_height();
                    let text_height = window.get_update_status_text_height();
                    if window.get_update_status().is_empty() {
                        assert_eq!(status_height, 0.0, "idle status row should collapse");
                    } else {
                        assert!(status_height > 0.0);
                        assert!(
                            (status_height - text_height).abs() < 0.1,
                            "the status row must contain the complete wrapped text without clipping for {distribution:?}/{locale:?}/{status:?}: row={status_height:.2}, text={text_height:.2}"
                        );
                    }
                    if matches!(status, AboutPreviewStatus::Long) && !store {
                        assert!(status_height > 32.0, "long status should grow naturally");
                    }
                    assert_eq!(
                        required,
                        window
                            .get_general_page_required_height()
                            .max(window.get_notifications_page_required_height())
                            .max(window.get_about_page_window_required_height())
                    );
                    if expected_window_size.is_none() {
                        window.set_settings_window_height(required);
                        window
                            .window()
                            .set_size(slint::LogicalSize::new(720.0, required));
                        expected_window_size = Some(window.window().size());
                    }
                    for tab in 0..3 {
                        window.invoke_select_settings_tab(tab);
                        assert_eq!(window.window().size(), expected_window_size.unwrap());
                        let viewport = window.get_settings_scroll_viewport_height();
                        let content_height = window.get_settings_scroll_content_height();
                        let scroll_extent = window.get_settings_scroll_max_extent();
                        let scroll_y_before = window.get_settings_scroll_y();
                        let button_y = window.get_settings_about_update_actions_y();
                        let status_y = window.get_settings_about_status_y();
                        let footer_bottom = window.get_settings_about_footer_bottom();
                        let logical_window_height = window
                            .window()
                            .size()
                            .to_logical(window.window().scale_factor().max(0.5))
                            .height;
                        assert!(
                            (viewport - logical_window_height).abs() < 0.1,
                            "Flickable viewport must match the Slint logical client height: viewport={viewport:.2}, logical_window={logical_window_height:.2}"
                        );
                        let status_expansion = window.get_settings_about_status_layout_expansion();
                        let reclaimed = window.get_settings_about_status_space_reclaimed();
                        assert_eq!(reclaimed, status_expansion.min(26.0));
                        for gap in [
                            window.get_settings_about_gap_after_update_actions(),
                            window.get_settings_about_gap_after_links(),
                            window.get_settings_about_gap_before_footer(),
                            window.get_settings_about_gap_after_divider(),
                        ] {
                            assert!((8.0..=16.0).contains(&gap));
                        }
                        if tab == 2 {
                            if let Some(expected) = expected_button_y {
                                assert!(
                                    (button_y - expected).abs() < 0.1,
                                    "About update button moved across status states: expected {expected:.2}, got {button_y:.2}"
                                );
                            } else {
                                expected_button_y = Some(button_y);
                            }
                            if let Some(expected) = expected_status_y {
                                assert!(
                                    (status_y - expected).abs() < 0.1,
                                    "About status row moved across status states"
                                );
                            } else {
                                expected_status_y = Some(status_y);
                            }
                            if store || !matches!(status, AboutPreviewStatus::Long) {
                                if let Some(expected) = expected_footer_bottom {
                                    assert!(
                                        (footer_bottom - expected).abs() < 0.1,
                                        "About footer moved across normal status states"
                                    );
                                } else {
                                    expected_footer_bottom = Some(footer_bottom);
                                }
                            }
                        }
                        let should_fit =
                            tab != 2 || store || !matches!(status, AboutPreviewStatus::Long);
                        if should_fit {
                            assert_eq!(
                                scroll_extent, 0.0,
                                "normal {status:?} content must fit in tab {tab} for {locale:?}/{distribution:?}"
                            );
                            if tab == 2 {
                                assert!(
                                    footer_bottom <= viewport + 0.1,
                                    "About footer must fit for {locale:?}/{distribution:?}/{status:?}: footer={footer_bottom:.1}, viewport={viewport:.1}"
                                );
                            }
                        } else {
                            assert!(
                                scroll_extent > 0.0,
                                "long text should retain real scrolling"
                            );
                        }
                        window.invoke_scroll_settings_to_bottom();
                        let scroll_y_after = window.get_settings_scroll_y();
                        assert!(
                            (scroll_y_after + scroll_extent).abs() < 0.1,
                            "scroll-to-bottom must expose the measured extent: extent={scroll_extent:.1}, y={scroll_y_after:.1}"
                        );
                        if should_fit {
                            assert_eq!(
                                scroll_y_after, 0.0,
                                "fitting content must not move on scroll"
                            );
                        } else if tab == 2 {
                            assert!(
                                window.get_settings_about_footer_bottom() <= viewport + 0.1,
                                "scrolling long text to the bottom must reveal the footer"
                            );
                        }
                        if locale == Locale::En && distribution == Distribution::GitHubPortable {
                            println!(
                                "GEOMETRY status={status:?} tab={tab} test_backend_window_size_px={:?} logical_viewport={viewport:.1} page_pref={:.1} shared_required={required:.1} top_padding={:.1} bottom_padding={:.1} content={content_height:.1} max_extent={scroll_extent:.1} content_y={scroll_y_before:.1}->{scroll_y_after:.1} button_y={button_y:.1} status_y={status_y:.1} status_h={status_height:.1} footer_bottom={footer_bottom:.1}",
                                window.window().size(),
                                window.get_active_page_required_height(),
                                window.get_settings_about_top_padding(),
                                window.get_settings_about_bottom_padding(),
                            );
                        }
                    }
                    for theme_kind in 0..=2 {
                        window.set_theme_kind(theme_kind);
                        window.invoke_apply_theme_palette();
                        assert_eq!(window.get_about_page_required_height(), about_height);
                        assert_eq!(window.get_about_update_button_gap(), 12.0);
                    }
                }
                // Reopening Settings after an exceptional saved result must
                // retain the normal-state sizing budget and native size.
                if store {
                    configure_about_update_ui(&window, distribution, locale, &UpdateStatus::Idle);
                } else {
                    apply_about_preview_status(&window, locale, AboutPreviewStatus::Long);
                }
                assert_eq!(
                    window.get_required_settings_height(),
                    expected_required_height.unwrap()
                );
                assert_eq!(window.window().size(), expected_window_size.unwrap());
                println!("{locale:?}/{distribution:?}: {measurements:?}");
            }
        }
    }

    #[test]
    fn localized_update_status_wraps_fully_on_narrow_about_content() {
        i_slint_backend_testing::init_no_event_loop();
        for locale in [Locale::Uk, Locale::En, Locale::Pl, Locale::Ru] {
            let window = SettingsWindow::new().unwrap();
            window
                .window()
                .set_size(slint::LogicalSize::new(420.0, 360.0));
            update_settings_window(
                &window,
                &AppSettings::default(),
                locale,
                false,
                &adapter::UiSnapshot::unavailable(),
                now_unix(),
            );
            configure_about_update_ui(
                &window,
                Distribution::GitHubPortable,
                locale,
                &UpdateStatus::Idle,
            );
            window.invoke_select_settings_tab(2);
            let idle_height = window.get_about_page_required_height();
            let native_size = window.window().size();
            let labels = locale.settings();
            let mut button_y: Option<f32> = None;

            for message in [
                labels.update_offline,
                labels.update_http,
                labels.update_invalid,
                labels.update_timeout,
            ] {
                window.set_update_status(message.into());
                window.invoke_select_settings_tab(2);
                let slot_height = window.get_update_status_slot_height();
                let text_height = window.get_update_status_text_height();
                assert!(slot_height > 0.0, "{locale:?}: status must remain visible");
                assert!(
                    (slot_height - text_height).abs() < 0.1,
                    "{locale:?}: wrapped status text must fit its row without clipping"
                );
                assert_eq!(window.get_about_update_button_gap(), 12.0);
                assert_eq!(window.window().size(), native_size);
                assert!(window.get_about_page_required_height() >= idle_height);
                let current_button_y = window.get_settings_about_update_actions_y();
                if let Some(expected) = button_y {
                    assert!((current_button_y - expected).abs() < 0.1);
                } else {
                    button_y = Some(current_button_y);
                }
                let scroll_extent = window.get_settings_scroll_max_extent();
                assert!(scroll_extent > 0.0, "constrained viewport should scroll");
                window.invoke_scroll_settings_to_bottom();
                let scroll_y = window.get_settings_scroll_y();
                assert!((scroll_y + scroll_extent).abs() < 0.1);
                assert!(
                    window.get_settings_about_footer_bottom()
                        <= window.get_settings_scroll_viewport_height() + 0.1,
                    "scrolling to the bottom must reveal the footer for {locale:?}"
                );
            }
        }
    }

    #[test]
    fn slint_keeps_inactive_settings_pages_measurable_at_their_actual_widths() {
        i_slint_backend_testing::init_no_event_loop();
        let window = SettingsWindow::new().unwrap();
        window.set_storage_warning("storage warning ".repeat(18).into());
        window.set_alerts_description("notification description ".repeat(12).into());
        window.set_legal_note("legal note ".repeat(120).into());

        let general = window.get_general_page_required_height();
        let notifications = window.get_notifications_page_required_height();
        let about = window.get_about_page_required_height();
        let required = window.get_required_settings_height();

        assert!(window.get_settings_content_width() > 0.0);
        assert!(
            general > 0.0,
            "general page has a non-zero measured width/height"
        );
        assert!(notifications > 0.0);
        assert!(about > 0.0);
        assert_ne!(
            general, notifications,
            "test pages must have distinct heights"
        );
        assert_ne!(
            notifications, about,
            "test pages must have distinct heights"
        );
        assert_ne!(general, about, "test pages must have distinct heights");
        assert_eq!(required, general.max(notifications).max(about));
        assert_eq!(about, required, "long About copy is the tallest page");

        window.set_settings_window_height(required);
        window
            .window()
            .set_size(slint::LogicalSize::new(720.0, required));
        let stable_size = window.window().size();

        for selected_tab in 0..3 {
            window.set_selected_tab(selected_tab);
            assert_eq!(window.get_general_page_required_height(), general);
            assert_eq!(
                window.get_notifications_page_required_height(),
                notifications
            );
            assert_eq!(window.get_about_page_required_height(), about);
            assert_eq!(window.get_required_settings_height(), required);
            assert_eq!(window.window().size(), stable_size);
        }

        // N1: the full About page scrolls inside a constrained viewport,
        // while the Window still retains the maximum height for all pages.
        let constrained_viewport = 420.0;
        window.set_settings_window_height(constrained_viewport);
        window
            .window()
            .set_size(slint::LogicalSize::new(720.0, constrained_viewport));
        window.invoke_select_settings_tab(2);
        assert_eq!(window.get_active_page_required_height(), about);
        assert_eq!(
            window.get_settings_scroll_content_height(),
            about.max(constrained_viewport)
        );
        assert_eq!(window.get_required_settings_height(), required);
        window.invoke_scroll_settings_to_bottom();
        assert!(window.get_settings_scroll_y() < 0.0);
        let about_scrolled_window_size = window.window().size();

        window.invoke_select_settings_tab(0);
        assert_eq!(window.get_settings_scroll_y(), 0.0);
        assert_eq!(window.get_active_page_required_height(), general);
        assert_eq!(
            window.get_settings_scroll_content_height(),
            general.max(constrained_viewport)
        );
        assert!(window.get_settings_scroll_content_height() < about);
        assert_eq!(window.get_required_settings_height(), required);
        assert_eq!(window.window().size(), about_scrolled_window_size);

        // Simulate a user-adjusted native size, then a setting/language/content
        // change. None of these UI updates may overwrite that manual size.
        window
            .window()
            .set_size(slint::LogicalSize::new(720.0, required - 24.0));
        let manually_resized_size = window.window().size();
        window.set_alerts_enabled(!window.get_alerts_enabled());
        window.set_legal_note("short legal note".into());
        assert!(window.get_required_settings_height() < required);
        let required_after_content_change = window.get_required_settings_height();
        assert_eq!(window.window().size(), manually_resized_size);

        // Status appears below the update action, expands the scroll extent,
        // and leaves both the action position and native window size intact.
        window.set_update_status(
            ("update available: ".to_owned() + &"localized release details ".repeat(10)).into(),
        );
        window.set_update_available(true);
        window.set_update_release_visible(true);
        assert_eq!(window.window().size(), manually_resized_size);
        let required_after_status = window.get_required_settings_height();
        assert_eq!(required_after_status, required_after_content_change);
        assert_eq!(window.get_about_update_button_gap(), 12.0);
        assert_eq!(
            window.get_update_status_slot_height(),
            window.get_update_status_text_height()
        );
        window.invoke_select_settings_tab(2);
        assert_eq!(window.window().size(), manually_resized_size);
        window.set_settings_window_height(constrained_viewport);
        window
            .window()
            .set_size(slint::LogicalSize::new(720.0, constrained_viewport));
        assert!(window.get_active_page_required_height() > constrained_viewport);
        window.invoke_scroll_settings_to_bottom();
        assert!(window.get_settings_scroll_y() < 0.0);
        window.set_settings_window_height(manually_resized_size.height as f32);
        window
            .window()
            .set_size(manually_resized_size.to_logical(1.0));
        assert_eq!(
            required_after_status,
            window
                .get_general_page_required_height()
                .max(window.get_notifications_page_required_height())
                .max(window.get_about_page_window_required_height())
        );

        // N2: the native size survives a DPI change when it fits the new work
        // area; only an oversized manual width/height is clamped.
        let fitting_size = window.window().size().to_logical(1.25);
        let changed_dpi_screen = screen(1.25);
        let retained =
            settings_geometry(fitting_size.width, fitting_size.height, changed_dpi_screen);
        assert_eq!(retained.width, fitting_size.width);
        assert_eq!(retained.height, fitting_size.height);
        assert_eq!(
            (retained.width * 1.25).round() as u32,
            manually_resized_size.width
        );
        assert_eq!(
            (retained.height * 1.25).round() as u32,
            manually_resized_size.height
        );
        assert_eq!(window.window().size(), manually_resized_size);

        let constrained_screen = SettingsScreenSignature {
            work_area: PhysicalWorkArea {
                left: 0,
                top: 0,
                right: 1300,
                bottom: 800,
            },
            ..changed_dpi_screen
        };
        let clamped = settings_geometry(1100.0, 700.0, constrained_screen);
        assert_eq!(clamped.width, (1300.0 - 2.0 * 16.0 * 1.25) / 1.25);
        assert_eq!(
            clamped.height,
            (800.0 - 2.0 * 16.0 * 1.25 - 32.0 * 1.25) / 1.25
        );
        window
            .window()
            .set_size(slint::LogicalSize::new(1100.0, 700.0));
        window
            .window()
            .set_size(slint::LogicalSize::new(clamped.width, clamped.height));
        assert_eq!(window.window().size().width as f32, clamped.width);
        assert_eq!(window.window().size().height as f32, clamped.height);
    }

    fn screen(scale: f32) -> SettingsScreenSignature {
        SettingsScreenSignature {
            monitor: 7,
            // A 1920x1040 work area represents a 1080px display with the
            // taskbar excluded; negative X models a monitor left of primary.
            work_area: PhysicalWorkArea {
                left: -1920,
                top: 0,
                right: 0,
                bottom: 1040,
            },
            scale_bits: scale.to_bits(),
        }
    }

    #[test]
    fn settings_height_is_capped_to_taskbar_excluded_work_area_at_common_dpis() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let screen = screen(scale);
            let geometry = settings_geometry(720.0, 1200.0, screen);
            let allowed_physical_height =
                1040 - 2 * (16.0 * scale).round() as i32 - (32.0 * scale).round() as i32;
            assert!(geometry.height <= allowed_physical_height as f32 / scale + 0.01);
            assert!(geometry.height >= geometry.minimum_height);
            assert!(geometry.x >= -1920);
            assert!(geometry.x < 0);
            assert!(geometry.y >= 0);
        }
    }

    #[test]
    fn fitting_manual_size_is_preserved_across_dpi_and_only_oversized_axes_clamp() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let screen = SettingsScreenSignature {
                work_area: PhysicalWorkArea {
                    left: -3840,
                    top: 0,
                    right: 0,
                    bottom: 2160,
                },
                ..screen(scale)
            };
            let geometry = settings_geometry(900.0, 700.0, screen);
            assert_eq!(geometry.width, 900.0);
            assert_eq!(geometry.height, 700.0);
        }

        let wide_but_short = SettingsScreenSignature {
            work_area: PhysicalWorkArea {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 800,
            },
            ..screen(1.0)
        };
        let height_only = settings_geometry(800.0, 900.0, wide_but_short);
        assert_eq!(height_only.width, 800.0);
        assert!(height_only.height < 900.0);

        let narrow_but_tall = SettingsScreenSignature {
            work_area: PhysicalWorkArea {
                left: 0,
                top: 0,
                right: 1000,
                bottom: 1400,
            },
            ..screen(1.0)
        };
        let width_only = settings_geometry(1100.0, 700.0, narrow_but_tall);
        assert!(width_only.width < 1100.0);
        assert_eq!(width_only.height, 700.0);
    }

    #[test]
    fn settings_geometry_keeps_content_scrollable_on_short_work_areas() {
        let mut screen = screen(2.0);
        screen.work_area.bottom = 600;
        let geometry = settings_geometry(720.0, 900.0, screen);
        assert!(geometry.height < 900.0);
        assert_eq!(geometry.minimum_height, geometry.height);
        assert!(geometry.y + (geometry.height * 2.0).round() as i32 + 64 <= 600);
    }

    /// Native message order for a user border drag.
    fn user_resize(ownership: &mut SettingsSizeOwnership, size: (f32, f32)) -> Option<(f32, f32)> {
        ownership.enter_size_move();
        ownership.user_sizing();
        ownership.user_sizing();
        ownership.exit_size_move(size)
    }

    #[test]
    fn auto_sized_settings_are_measured_again_after_close_and_reopen() {
        let ownership = SettingsSizeOwnership::new();
        // First open: the app sizes the window (no native size loop), the
        // asynchronous resize lands, and the user closes Settings.
        assert_eq!(
            settings_open_size(720.0, 748.0, ownership.user_size()),
            (720.0, 748.0)
        );
        assert_eq!(ownership.user_size(), None);
        // Reopen: nothing was retained, so the post-show fit runs again.
        assert!(native_first_show_remeasure_allowed(
            ownership.user_size().is_none(),
            &native_first_show_app_owned_sizes((720.0, 748.0), Some((720.0, 748.0))),
            (720.0, 748.0),
            false,
        ));
    }

    #[test]
    fn language_or_content_growth_between_opens_refits_an_automatic_size() {
        let ownership = SettingsSizeOwnership::new();
        // About grew between opens (e.g. a longer locale): 738 -> 764.
        assert_eq!(
            settings_open_size(720.0, 738.0, ownership.user_size()),
            (720.0, 738.0)
        );
        assert_eq!(
            settings_open_size(720.0, 764.0, ownership.user_size()),
            (720.0, 764.0)
        );
        let roomy = SettingsScreenSignature {
            work_area: PhysicalWorkArea {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1032,
            },
            ..screen(1.0)
        };
        let fitted = settings_geometry(720.0, 764.0, roomy);
        assert_eq!(fitted.height, 764.0);
    }

    #[test]
    fn manual_resize_persists_across_close_and_reopen() {
        let mut ownership = SettingsSizeOwnership::new();
        assert_eq!(
            user_resize(&mut ownership, (880.0, 680.0)),
            Some((880.0, 680.0))
        );
        // Close does not change it; reopen uses it even when content grew.
        let retained = ownership.user_size();
        assert_eq!(retained, Some((880.0, 680.0)));
        assert_eq!(settings_open_size(720.0, 764.0, retained), (880.0, 680.0));
        // A retained user size blocks the automatic post-show fit.
        assert!(!native_first_show_remeasure_allowed(
            retained.is_none(),
            &native_first_show_app_owned_sizes((880.0, 680.0), Some((880.0, 680.0))),
            (880.0, 680.0),
            false,
        ));
        // A later move-only loop keeps the user's size.
        ownership.enter_size_move();
        assert_eq!(ownership.exit_size_move((880.0, 680.0)), None);
        assert_eq!(ownership.user_size(), Some((880.0, 680.0)));
        // A second manual resize replaces it.
        assert_eq!(
            user_resize(&mut ownership, (760.0, 700.0)),
            Some((760.0, 700.0))
        );
        assert_eq!(ownership.user_size(), Some((760.0, 700.0)));
    }

    #[test]
    fn only_a_native_user_size_loop_marks_the_size_as_user_selected() {
        let mut ownership = SettingsSizeOwnership::new();
        // App/Winit `SetWindowPos`, DPI change, maximize and snap: no size
        // loop with WM_SIZING, whatever the client size becomes.
        assert_eq!(ownership.user_size(), None);
        // Dragging the caption (move loop, no WM_SIZING), including moving to
        // another monitor with a different DPI, is not a resize.
        ownership.enter_size_move();
        assert_eq!(ownership.exit_size_move((960.0, 1000.0)), None);
        // A stray WM_SIZING outside a loop is ignored.
        ownership.user_sizing();
        assert_eq!(ownership.exit_size_move((960.0, 1000.0)), None);
        // A loop that ends with an unusable size records nothing.
        ownership.enter_size_move();
        ownership.user_sizing();
        assert_eq!(ownership.exit_size_move((0.0, 0.0)), None);
        assert_eq!(ownership.user_size(), None);
        // Flags reset after each loop.
        ownership.enter_size_move();
        assert_eq!(ownership.exit_size_move((720.0, 748.0)), None);
        assert_eq!(ownership.user_size(), None);
    }

    #[test]
    fn post_show_fit_uses_the_monitor_containing_settings() {
        // Hidden: open on the cursor's monitor. Visible (the 150 ms post-show
        // fit and monitor-change refresh): the window's own monitor.
        assert_eq!(settings_fit_monitor(false), SettingsMonitor::Cursor);
        assert_eq!(settings_fit_monitor(true), SettingsMonitor::Window);
    }

    #[test]
    fn moving_settings_between_monitors_keeps_work_area_and_dpi_bounds() {
        let large = SettingsScreenSignature {
            monitor: 1,
            work_area: PhysicalWorkArea {
                left: 0,
                top: 0,
                right: 2560,
                bottom: 1392,
            },
            scale_bits: 1.5f32.to_bits(),
        };
        let small = SettingsScreenSignature {
            monitor: 2,
            work_area: PhysicalWorkArea {
                left: 2560,
                top: 0,
                right: 3926,
                bottom: 728,
            },
            scale_bits: 1.0f32.to_bits(),
        };
        // An automatic size clamped on the small monitor is not retained, so
        // a later open on the large monitor gets the full required height.
        let ownership = SettingsSizeOwnership::new();
        let on_small = settings_geometry(720.0, 748.0, small);
        assert!(on_small.height < 748.0);
        assert!(on_small.minimum_height <= on_small.height);
        let (width, height) = settings_open_size(720.0, 748.0, ownership.user_size());
        let on_large = settings_geometry(width, height, large);
        assert_eq!((on_large.width, on_large.height), (720.0, 748.0));
        assert!(on_large.x >= large.work_area.left && on_large.x < large.work_area.right);

        // A user size from the large monitor is clamped on the small one and
        // restored in full when the large monitor is used again.
        let mut ownership = SettingsSizeOwnership::new();
        user_resize(&mut ownership, (900.0, 820.0));
        let (width, height) = settings_open_size(720.0, 748.0, ownership.user_size());
        let clamped = settings_geometry(width, height, small);
        assert_eq!(clamped.width, 900.0);
        assert!(clamped.height < 820.0);
        assert!(clamped.x >= small.work_area.left);
        let restored = settings_geometry(width, height, large);
        assert_eq!((restored.width, restored.height), (900.0, 820.0));
    }

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
        let store_settings = |claude_enabled, bridge| AppSettings {
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
        // EXE builds use their own persisted choice plus the session-only flag.
        assert!(!claude_bridge_wanted_for(
            false,
            false,
            &store_settings(true, true)
        ));
        assert!(claude_bridge_wanted_for(
            false,
            true,
            &store_settings(true, false)
        ));
        let exe_opted_in = AppSettings {
            claude_enabled: true,
            exe_claude_bridge_enabled: Some(true),
            ..Default::default()
        };
        assert!(claude_bridge_wanted_for(false, false, &exe_opted_in));
        let exe_opted_out = AppSettings {
            claude_enabled: true,
            exe_claude_bridge_enabled: Some(false),
            ..Default::default()
        };
        assert!(!claude_bridge_wanted_for(false, false, &exe_opted_out));
        // The session-only CLI flag overrides saved OFF without changing it.
        assert!(claude_bridge_wanted_for(false, true, &exe_opted_out));
        assert_eq!(exe_opted_out.exe_claude_bridge_enabled, Some(false));
        assert!(exe_bridge_cli_override_active(true, &exe_opted_out));
        assert_eq!(exe_bridge_toggle_state(true, &exe_opted_out), (true, true));
        assert_eq!(
            exe_bridge_toggle_state(false, &exe_opted_out),
            (false, false)
        );
        let unset_choice = AppSettings {
            claude_enabled: true,
            ..Default::default()
        };
        assert!(claude_bridge_wanted_for(false, true, &unset_choice));
        assert!(exe_bridge_cli_override_active(true, &unset_choice));
        // MSIX build: the persisted setting is the opt-in.
        assert!(claude_bridge_wanted_for(
            true,
            false,
            &store_settings(true, true)
        ));
        assert!(!claude_bridge_wanted_for(
            true,
            true,
            &store_settings(true, false)
        ));
        assert!(!claude_bridge_wanted_for(
            true,
            false,
            &store_settings(true, false)
        ));
        // Claude off stops the listener in every build, whatever the opt-in.
        for packaged in [false, true] {
            for argument in [false, true] {
                assert!(!claude_bridge_wanted_for(
                    packaged,
                    argument,
                    &store_settings(false, true)
                ));
            }
        }
        // Tests never run packaged, so the runtime helper follows the EXE rules.
        assert!(!claude_bridge_wanted(false, &store_settings(true, true)));
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
