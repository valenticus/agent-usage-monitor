mod adapter;
mod claude_refresher;
mod localization;
mod provider_icons;
mod settings;
mod theme;
#[allow(dead_code)] // Pure quota renderer is tested; the native tray icon is currently static.
mod tray;

slint::include_modules!();

use std::path::Path;
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

const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
type SharedSettings = Arc<Mutex<AppSettings>>;
type SharedLocale = Arc<Mutex<Locale>>;
type SharedSnapshot = Arc<Mutex<adapter::UiSnapshot>>;
type SharedAlerts = Arc<Mutex<AlertRuntime>>;

struct AlertRuntime {
    engine: AlertEngine,
    state_path: std::path::PathBuf,
}

impl AlertRuntime {
    fn new(settings: &AppSettings, state_path: std::path::PathBuf) -> Self {
        let state = AlertStateStore::load(&state_path).unwrap_or_default();
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
        let _ = AlertStateStore::save(&self.state_path, self.engine.state());
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
    };
    let message = match locale {
        Locale::Uk => format!("Залишилося {}%", event.remaining_percent),
        Locale::En => format!("{}% remaining", event.remaining_percent),
        Locale::Pl => format!("Pozostało {}%", event.remaining_percent),
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
    let args: Vec<String> = std::env::args().collect();
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
    let settings_path = settings::settings_path();
    if settings::migrate_legacy_data(
        &settings::legacy_settings_directory(),
        &settings::settings_directory(),
    )
    .is_err()
    {
        eprintln!("Settings migration could not complete; existing data was retained.");
    }
    let config: SharedSettings = Arc::new(Mutex::new(
        settings::load(&settings_path).unwrap_or_default(),
    ));
    let locale: SharedLocale = Arc::new(Mutex::new(resolve_locale(&lock_settings(&config))));
    let initial_snapshot = initial_snapshot(demo_mode, use_live, claude_bridge_enabled, now_unix());
    let current: SharedSnapshot = Arc::new(Mutex::new(initial_snapshot.clone()));
    let alerts: SharedAlerts = Arc::new(Mutex::new(AlertRuntime::new(
        &lock_settings(&config),
        settings_path.with_file_name("alert-state.json"),
    )));

    let window = MainWindow::new()?;
    let settings_window = SettingsWindow::new()?;
    window.set_refresh_enabled(use_live);
    window
        .window()
        .on_close_requested(|| slint::CloseRequestResponse::HideWindow);
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
    window.on_refresh_requested(move || {
        let _ = refresh_sender.try_send(());
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

    #[cfg(windows)]
    let (native_tray, tray_events) = match tray::NativeTray::start(lock_locale(&locale)) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("Windows tray initialization failed: {error}. The application will exit.");
            return Ok(());
        }
    };
    #[cfg(windows)]
    let worker_notifier = Some(native_tray.notifier());
    #[cfg(not(windows))]
    let worker_notifier = None;
    #[cfg(windows)]
    let _tray_timer = {
        let popup = window.as_weak();
        let settings = settings_window.as_weak();
        let tray_timer = slint::Timer::default();
        let events = tray_events;
        tray_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(50),
            move || {
                while let Ok(event) = events.try_recv() {
                    match event {
                        tray::TrayEvent::TogglePopup => {
                            if let Some(popup) = popup.upgrade() {
                                if popup_visible_after_toggle(popup.window().is_visible()) {
                                    let _ = popup.show();
                                } else {
                                    let _ = popup.hide();
                                }
                            }
                        }
                        tray::TrayEvent::OpenSettings => {
                            if let Some(settings) = settings.upgrade() {
                                show_settings_centered(&settings);
                            }
                        }
                        tray::TrayEvent::Quit => {
                            let _ = slint::quit_event_loop();
                        }
                    }
                }
            },
        );
        tray_timer
    };

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

    let theme_window = window.as_weak();
    let theme_panel = settings_window.as_weak();
    let theme_config = config.clone();
    let theme_locale = locale.clone();
    let theme_snapshot = current.clone();
    let theme_timer = slint::Timer::default();
    theme_timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(2),
        move || {
            let settings = lock_settings(&theme_config);
            let accent = theme::system_accent_color();
            if let Some(window) = theme_window.upgrade() {
                window.set_accent_color(accent);
            }
            if let Some(panel) = theme_panel.upgrade() {
                panel.set_accent_color(accent);
            }
            if settings.theme_mode != ThemeMode::System {
                return;
            }
            if let (Some(window), Ok(snapshot)) = (theme_window.upgrade(), theme_snapshot.lock()) {
                adapter::apply_config(
                    &window,
                    &snapshot,
                    now_unix(),
                    &settings,
                    lock_locale(&theme_locale),
                    theme::system_prefers_light(),
                );
            }
            if let Some(panel) = theme_panel.upgrade() {
                if let Ok(snapshot) = theme_snapshot.lock() {
                    update_settings_window(
                        &panel,
                        &settings,
                        lock_locale(&theme_locale),
                        theme::system_prefers_light(),
                        &snapshot,
                        now_unix(),
                    );
                }
            }
        },
    );

    #[cfg(windows)]
    let _popup_style_timer = {
        let popup = window.as_weak();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(50),
            move || {
                let Some(popup) = popup.upgrade() else {
                    return;
                };
                let _ = apply_popup_taskbar_style(&popup);
            },
        );
        timer
    };

    let result = window.run();
    cancellation.cancel();
    if let Some(worker) = worker {
        let _ = worker.join();
    }
    result
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
    let (config, path, locale, sender, window, panel, snapshot) = context;
    let mut updated = lock_settings(config);
    change(&mut updated);
    if settings::save(path, &updated).is_err() {
        return;
    }
    if let Ok(mut current) = config.lock() {
        *current = updated.clone();
    }
    let resolved = resolve_locale(&updated);
    if let Ok(mut current_locale) = locale.lock() {
        *current_locale = resolved;
    }
    let _ = sender.send(updated.clone());
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
    refresh_requests: mpsc::Receiver<()>,
) where
    P: RateLimitProvider + 'static,
{
    let (sender, mut receiver) = mpsc::channel::<ClaudeStatusLineSnapshot>(4);
    let (cli_sender, mut cli_receiver) =
        mpsc::channel::<agent_usage_monitor_core::ClaudeUsageSnapshot>(2);
    let claude_window = context.window.clone();
    let claude_snapshot = context.current.clone();
    let claude_config = context.config.clone();
    let claude_locale = context.locale.clone();
    let claude_cancellation = cancellation.clone();
    let claude_settings = settings.clone();
    let cli_settings = settings.clone();
    let pipe_wait_cancellation = cancellation.clone();
    let claude_alerts = context.alerts.clone();
    let claude_settings_window = context.settings_window.clone();
    let claude_notifier = context.notifier.clone();
    let claude_updates = async move {
        let mut state = ProviderState::new("claude");
        loop {
            tokio::select! {
                _ = claude_cancellation.cancelled() => break,
                snapshot = receiver.recv(), if claude_bridge_enabled => {
                    let Some(snapshot) = snapshot else { continue };
                    if !claude_settings.borrow().claude_enabled { continue; }
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
                        if let Some(window) = claude_window.upgrade() { apply_current(&window, &updated, &claude_config, &claude_locale); }
                        if let Some(panel) = claude_settings_window.upgrade() {
                            let settings = lock_settings(&claude_config);
                            update_settings_window(&panel, &settings, lock_locale(&claude_locale), system_light(&settings), &updated, now_unix());
                        }
                    }
                }
                usage = cli_receiver.recv() => {
                    let Some(usage) = usage else { continue };
                    if !claude_settings.borrow().claude_enabled
                        || !claude_settings.borrow().claude_cli_refresh_enabled
                    { continue; }
                    let updated = if let Ok(mut current) = claude_snapshot.lock() {
                        *current = current.merge_claude_usage(usage);
                        Some(current.clone())
                    } else { None };
                    if let Some(updated) = updated {
                        if let Ok(mut alert_runtime) = claude_alerts.lock() {
                            let events = alert_runtime.refresh(&updated, &lock_settings(&claude_config), now_unix());
                            deliver_alerts(&claude_notifier, events, lock_locale(&claude_locale));
                        }
                        if let Some(window) = claude_window.upgrade() {
                            apply_current(&window, &updated, &claude_config, &claude_locale);
                        }
                        if let Some(panel) = claude_settings_window.upgrade() {
                            let settings = lock_settings(&claude_config);
                            update_settings_window(&panel, &settings, lock_locale(&claude_locale), system_light(&settings), &updated, now_unix());
                        }
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
    let pipe_cancellation = cancellation.clone();
    let pipe_updates = async move {
        if !claude_bridge_enabled {
            pipe_wait_cancellation.cancelled().await;
            return;
        }
        if run_claude_status_pipe(sender, pipe_cancellation.clone())
            .await
            .is_err()
        {
            if let Ok(mut current) = pipe_snapshot.lock() {
                *current = current.replace_claude_error();
            }
            if let (Some(window), Ok(current)) = (pipe_window.upgrade(), pipe_snapshot.lock()) {
                apply_current(&window, &current, &pipe_config, &pipe_locale);
            }
            if let (Some(panel), Ok(current)) =
                (pipe_settings_window.upgrade(), pipe_snapshot.lock())
            {
                let settings = lock_settings(&pipe_config);
                update_settings_window(
                    &panel,
                    &settings,
                    lock_locale(&pipe_locale),
                    system_light(&settings),
                    &current,
                    now_unix(),
                );
            }
            pipe_cancellation.cancelled().await;
        }
    };
    let codex = refresh_loop(
        provider,
        context,
        cancellation.clone(),
        settings,
        refresh_requests,
    );
    let cli = claude_refresher::run(cli_settings, cli_sender, cancellation);
    tokio::join!(codex, claude_updates, pipe_updates, cli);
}

async fn refresh_loop<P>(
    provider: P,
    context: WorkerUiContext,
    cancellation: CancellationToken,
    mut settings: watch::Receiver<AppSettings>,
    mut refresh_requests: mpsc::Receiver<()>,
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
                    refresh_codex(&provider, &mut state, &context, &cancellation).await;
                } else { enabled = next; }
            },
            _ = interval.tick(), if enabled => {
                refresh_codex(&provider, &mut state, &context, &cancellation).await;
            }
            request = refresh_requests.recv(), if enabled => {
                if request.is_none() { break; }
                refresh_codex(&provider, &mut state, &context, &cancellation).await;
            }
        }
    }
}

async fn refresh_codex<P: RateLimitProvider>(
    provider: &P,
    state: &mut ProviderState,
    context: &WorkerUiContext,
    cancellation: &CancellationToken,
) {
    let result = provider.read_rate_limits(cancellation.child_token()).await;
    match result {
        Ok(limits) => state.record_success(limits, None, now_unix()),
        Err(error) if error.code == "codex_cancelled" && cancellation.is_cancelled() => return,
        Err(error) => state.record_failure(status_for(&error), error),
    }
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
        if let Some(window) = context.window.upgrade() {
            apply_current(&window, &updated, &context.config, &context.locale);
        }
        if let Some(panel) = context.settings_window.upgrade() {
            let settings = lock_settings(&context.config);
            update_settings_window(
                &panel,
                &settings,
                lock_locale(&context.locale),
                system_light(&settings),
                &updated,
                now_unix(),
            );
        }
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
    let icons = provider_icons::local();
    window.set_codex_icon(icons.codex.clone().unwrap_or_default());
    window.set_codex_icon_available(icons.codex.is_some());
    window.set_claude_icon(icons.claude.clone().unwrap_or_default());
    window.set_claude_icon_available(icons.claude.is_some());
    window.set_alerts_enabled(settings.alerts_enabled);
    window.set_exhausted_alerts_enabled(settings.exhausted_alerts_enabled);
    window.set_include_weekly(settings.include_weekly);
    window.set_theme_kind(adapter::theme_kind(settings.theme_mode, light));
    window.invoke_apply_theme_palette();
    window.set_accent_color(theme::system_accent_color());
    window.set_theme_options(
        [
            SharedString::from(labels.system),
            SharedString::from(labels.graphite),
            SharedString::from(labels.navy),
            SharedString::from(labels.light),
        ]
        .into(),
    );
    window.set_theme_index(match settings.theme_mode {
        ThemeMode::System => 0,
        ThemeMode::Graphite => 1,
        ThemeMode::Navy => 2,
        ThemeMode::Light => 3,
    });
    window.set_language_options(
        [
            SharedString::from(labels.system_language),
            "Українська".into(),
            "English".into(),
            "Polski".into(),
        ]
        .into(),
    );
    window.set_language_index(match settings.language {
        Language::System => 0,
        Language::Ukrainian => 1,
        Language::English => 2,
        Language::Polish => 3,
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
    let (claude_state, claude_description) =
        snapshot.settings_status("claude", settings.claude_enabled, now, locale);
    window.set_claude_state(claude_state.into());
    window.set_claude_description(claude_description.into());

    window.set_title_label(labels.title.into());
    window.set_general_label(labels.general.into());
    window.set_notifications_label(labels.notifications.into());
    window.set_about_label(labels.about.into());
    window.set_appearance_label(labels.appearance.into());
    window.set_providers_label(labels.providers.into());
    window.set_theme_name_label(labels.theme.into());
    window.set_language_name_label(labels.language.into());
    window.set_warning_name_label(labels.warning.into());
    window.set_critical_name_label(labels.critical.into());
    window.set_exhausted_name_label(labels.exhausted.into());
    window.set_weekly_name_label(labels.weekly.into());
    window.set_duration_name_label(labels.duration.into());
    window.set_alerts_description(labels.alerts_description.into());
    window.set_source_note(labels.claude_note.into());
    window.set_about_product("AgentUsageMonitor".into());
    window.set_about_tagline(labels.about_tagline.into());
    window.set_claude_refresh_label(labels.claude_refresh.into());
    window.set_claude_refresh_description(labels.claude_refresh_description.into());
    window.set_about_version(format!("{}  {}", labels.version, env!("CARGO_PKG_VERSION")).into());
    window.set_about_author(format!("{}  Valentyn Fetisov", labels.author).into());
    window.set_about_repository(labels.repository.into());
    window.set_about_license(labels.license.into());
    window.set_about_technologies(labels.technologies.into());
}

fn show_settings_centered(window: &SettingsWindow) {
    let _ = window.show();
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
                    let size = window.window().size();
                    let area = monitor_info.rcWork;
                    let x = area.left + ((area.right - area.left - size.width as i32) / 2);
                    let y = area.top + ((area.bottom - area.top - size.height as i32) / 2);
                    window
                        .window()
                        .set_position(slint::PhysicalPosition::new(x, y));
                }
            }
        }
    }
}

fn open_repository() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        let operation: Vec<u16> = "open\0".encode_utf16().collect();
        let url: Vec<u16> = "https://github.com/valenticus/agent-usage-monitor\0"
            .encode_utf16()
            .collect();
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
        ThemeMode::Graphite | ThemeMode::Navy => false,
        ThemeMode::Light => true,
        ThemeMode::System => theme::system_prefers_light(),
    }
}

fn resolve_locale(settings: &AppSettings) -> Locale {
    let system = localization::system_language();
    Locale::resolve(settings.language, system.as_deref())
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

fn popup_visible_after_toggle(currently_visible: bool) -> bool {
    !currently_visible
}
fn set_theme(settings: &mut AppSettings, value: &str, locale: Locale) {
    let labels = locale.settings();
    settings.theme_mode = if value == labels.graphite {
        ThemeMode::Graphite
    } else if value == labels.navy {
        ThemeMode::Navy
    } else if value == labels.light {
        ThemeMode::Light
    } else {
        ThemeMode::System
    };
}
fn set_language(settings: &mut AppSettings, value: &str, locale: Locale) {
    settings.language = if value == "Українська" {
        Language::Ukrainian
    } else if value == "English" {
        Language::English
    } else if value == "Polski" {
        Language::Polish
    } else if value == locale.settings().system_language {
        Language::System
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
            theme_mode: ThemeMode::Navy,
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
        set_theme(&mut settings, "Темно-синя", Locale::Uk);
        assert_eq!(settings.theme_mode, ThemeMode::Navy);
        set_theme(&mut settings, "Світла", Locale::Uk);
        assert_eq!(settings.theme_mode, ThemeMode::Light);
        set_theme(&mut settings, "Системна", Locale::Uk);
        assert_eq!(settings.theme_mode, ThemeMode::System);

        set_language(&mut settings, "Українська", Locale::En);
        assert_eq!(settings.language, Language::Ukrainian);
        set_language(&mut settings, "English", Locale::Uk);
        assert_eq!(settings.language, Language::English);
        set_language(&mut settings, "Polski", Locale::En);
        assert_eq!(settings.language, Language::Polish);
        set_language(&mut settings, "System", Locale::En);
        assert_eq!(settings.language, Language::System);
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
    fn tray_popup_toggle_switches_between_open_and_hidden_states() {
        assert!(popup_visible_after_toggle(false));
        assert!(!popup_visible_after_toggle(true));
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
