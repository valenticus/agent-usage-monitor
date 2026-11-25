//! Flyout window state machine.
//!
//! Each controller owns one `MainWindow` instance. The main and compact
//! preview windows have separate lifecycle state and share data in `main.rs`.
//!
//! Hide timing uses deadlines that are checked by a single poll (`tick`, driven
//! by the existing 50 ms UI timer), so there are no free-running per-event
//! timers. Animations carry a generation token: a newer show/hide/transition
//! invalidates an older one, so an obsolete fade can never hide a window that
//! was re-shown or pinned in the meantime.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::ComponentHandle;

use crate::MainWindow;
use crate::tray::IconRect;

const SHOW_DURATION: Duration = Duration::from_millis(180);
const HIDE_DURATION: Duration = Duration::from_millis(150);
const LEAVE_DELAY: Duration = Duration::from_secs(2);
const REFRESH_HOLD: Duration = Duration::from_secs(5);
/// How long the cursor must rest on the tray icon (Windows default hover time).
const HOVER_DELAY: Duration = Duration::from_millis(400);
/// Safety net: a temporary preview never waits longer than this for a refresh.
const REFRESH_TIMEOUT: Duration = Duration::from_secs(60);
const FRAME: Duration = Duration::from_millis(15);
/// Vertical slide distance of the show/hide animation, logical pixels.
const SLIDE: f32 = 6.0;
/// Distance between the flyout and the taskbar / screen edge, logical pixels.
const EDGE_GAP: f32 = 12.0;
/// Flyout outer corner radius, logical pixels (spec: 16).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Hidden,
    CompactTemporary,
    FullPinned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowRole {
    Main,
    Preview,
}

fn hover_decision(role: WindowRole, mode: Mode, main_is_foreground: bool) -> Option<HoverAction> {
    (role == WindowRole::Preview).then(|| hover_action(mode, main_is_foreground))
}

fn compact_presentation(role: WindowRole) -> bool {
    role == WindowRole::Preview
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

pub struct Flyout {
    window: slint::Weak<MainWindow>,
    role: WindowRole,
    /// Read-only foreground eligibility check; preview operations never write here.
    main_window: Option<slint::Weak<MainWindow>>,
    icon_rect: Box<dyn Fn() -> Option<IconRect>>,
    mode: Cell<Mode>,
    /// When the temporary preview should start hiding (cursor left it).
    hide_deadline: Cell<Option<Instant>>,
    /// After a context-menu refresh: visible until this instant unless the
    /// user interacts (then the normal 2-second leave rule applies). Hover
    /// events never shorten it.
    hold_until: Cell<Option<Instant>>,
    /// First pointer movement over the tray icon while a preview is eligible.
    hover_since: Cell<Option<Instant>>,
    /// Set while a context-menu refresh is running; the preview stays visible.
    refresh_started: Cell<Option<Instant>>,
    /// Last position of the pinned flyout (kept across close/open).
    pinned_position: Cell<Option<slint::PhysicalPosition>>,
    /// Target on-screen rectangle of the current presentation.
    target: Cell<Option<Rect>>,
    /// DWM corner preference key (hwnd, width, height) last applied.
    corner_key: Cell<Option<(isize, i32, i32)>>,
    hiding: Cell<bool>,
    generation: Cell<u64>,
    animation: slint::Timer,
    /// Native TOPMOST band currently applied (see `sync_native_style`).
    topmost: Cell<bool>,
    /// Last known `WS_EX_NOACTIVATE` state; the live style is re-read on every
    /// sync because winit rewrites it (see `sync_native_style`).
    no_activate: Cell<bool>,
    /// `WS_EX_NOACTIVATE` writes in the current presentation (reset by
    /// `present`); bounds re-application if something keeps resetting it.
    no_activate_writes: Cell<u32>,
    /// A failed native style change was already logged (once per failure run).
    style_error_logged: Cell<bool>,
}

/// Most `WS_EX_NOACTIVATE` writes per presentation: one before the show, one
/// after winit's post-show style rewrite, with headroom for a hide/show pair.
const MAX_NO_ACTIVATE_WRITES: u32 = 4;

/// What to do with the activation style on one sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StyleAction {
    /// The live style already matches.
    Keep,
    /// Write the wanted style.
    Apply,
    /// Something keeps resetting it: stop writing for this presentation.
    GiveUp,
}

/// Decides from the *live* style, never only from what AUM last wrote: winit
/// 0.30.13 rewrites `GWL_EXSTYLE` from its own flags right after
/// `ShowWindow` (`window_state.rs:399–407`), silently dropping
/// `WS_EX_NOACTIVATE`. Writes are bounded per presentation so a style that
/// keeps being reset cannot cause a write on every 50 ms tick.
fn no_activate_action(live: bool, wanted: bool, writes: u32) -> StyleAction {
    if live == wanted {
        StyleAction::Keep
    } else if writes < MAX_NO_ACTIVATE_WRITES {
        StyleAction::Apply
    } else {
        StyleAction::GiveUp
    }
}

/// Native window state wanted for a presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeStyle {
    /// In the TOPMOST band (above the foreground app).
    topmost: bool,
    /// `WS_EX_NOACTIVATE`: showing or clicking never activates the window.
    no_activate: bool,
}

/// Window rules per presentation:
/// - the compact (hover / temporary) preview never takes activation, so the
///   foreground app keeps the keyboard focus. winit shows every window after
///   its first show with `SW_SHOW`, which activates whenever Windows allows
///   it, so the style must be in place *before* the window is shown;
/// - the visible compact preview is TOPMOST; a fading-out one keeps the band
///   until it is actually hidden;
/// - the main statistics window is a normal, activatable window, never TOPMOST.
fn native_style(role: WindowRole, visible: bool) -> NativeStyle {
    let preview = role == WindowRole::Preview;
    NativeStyle {
        topmost: visible && preview,
        no_activate: preview,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoverAction {
    KeepFull,
    KeepCompact,
    ShowCompact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoverTimerAction {
    Cancel,
    Wait,
    Dispatch,
}

fn next_hover_since(
    pending_since: Option<Instant>,
    candidate: bool,
    now: Instant,
) -> Option<Instant> {
    candidate.then_some(pending_since.unwrap_or(now))
}

fn pointer_hover_candidate(mode: Mode, full_window_is_foreground: bool) -> bool {
    match mode {
        Mode::Hidden => !full_window_is_foreground,
        Mode::FullPinned => !full_window_is_foreground,
        Mode::CompactTemporary => false,
    }
}

fn hover_timer_action(
    mode: Mode,
    full_window_is_foreground: bool,
    pointer_is_over_icon: bool,
    elapsed: Duration,
) -> HoverTimerAction {
    if !pointer_hover_candidate(mode, full_window_is_foreground) || !pointer_is_over_icon {
        HoverTimerAction::Cancel
    } else if elapsed >= HOVER_DELAY {
        HoverTimerAction::Dispatch
    } else {
        HoverTimerAction::Wait
    }
}

fn hover_action(mode: Mode, full_window_is_foreground: bool) -> HoverAction {
    match mode {
        Mode::FullPinned if full_window_is_foreground => HoverAction::KeepFull,
        Mode::Hidden if full_window_is_foreground => HoverAction::KeepFull,
        Mode::FullPinned | Mode::Hidden => HoverAction::ShowCompact,
        Mode::CompactTemporary => HoverAction::KeepCompact,
    }
}

impl Flyout {
    pub fn new(
        window: &MainWindow,
        icon_rect: impl Fn() -> Option<IconRect> + 'static,
    ) -> Rc<Self> {
        Self::new_with_main(window, None, icon_rect)
    }

    pub fn new_with_main(
        window: &MainWindow,
        main_window: Option<&MainWindow>,
        icon_rect: impl Fn() -> Option<IconRect> + 'static,
    ) -> Rc<Self> {
        Rc::new(Self {
            window: window.as_weak(),
            role: if main_window.is_some() {
                WindowRole::Preview
            } else {
                WindowRole::Main
            },
            main_window: main_window.map(MainWindow::as_weak),
            icon_rect: Box::new(icon_rect),
            mode: Cell::new(Mode::Hidden),
            hide_deadline: Cell::new(None),
            hold_until: Cell::new(None),
            hover_since: Cell::new(None),
            refresh_started: Cell::new(None),
            pinned_position: Cell::new(None),
            target: Cell::new(None),
            corner_key: Cell::new(None),
            hiding: Cell::new(false),
            generation: Cell::new(0),
            animation: slint::Timer::default(),
            topmost: Cell::new(false),
            no_activate: Cell::new(false),
            no_activate_writes: Cell::new(0),
            style_error_logged: Cell::new(false),
        })
    }

    /// The single place that changes the native window style. Called right
    /// after the presentation is chosen (before any show or activation), after
    /// every show/hide, and from `tick` as a safety net, so no path can leave
    /// the window TOPMOST or non-activatable in the wrong mode. A no-op unless
    /// the wanted state differs; never activates or moves the window.
    fn sync_native_style(&self, window: &MainWindow) {
        let wanted = native_style(self.role, window.window().is_visible());
        // Activation style first: it must be set before a compact show and
        // cleared before the full flyout is activated. Compare with the live
        // style: winit drops WS_EX_NOACTIVATE right after every ShowWindow,
        // and the next sync (at the latest the 50 ms tick) restores it.
        let live = read_no_activate(window).unwrap_or(self.no_activate.get());
        self.no_activate.set(live);
        match no_activate_action(live, wanted.no_activate, self.no_activate_writes.get()) {
            StyleAction::Keep => {}
            StyleAction::Apply => {
                self.no_activate_writes
                    .set(self.no_activate_writes.get() + 1);
                match set_no_activate(window, wanted.no_activate) {
                    Ok(()) => self.no_activate.set(wanted.no_activate),
                    Err(error) => self.log_style_error("no_activate", wanted.no_activate, &error),
                }
            }
            StyleAction::GiveUp => self.log_style_error(
                "no_activate",
                wanted.no_activate,
                "the style keeps being reset; not retrying until the next presentation",
            ),
        }
        if wanted.topmost != self.topmost.get() {
            match set_topmost(window, wanted.topmost) {
                Ok(()) => self.topmost.set(wanted.topmost),
                Err(error) => self.log_style_error("topmost", wanted.topmost, &error),
            }
        }
        if self.no_activate.get() == wanted.no_activate && self.topmost.get() == wanted.topmost {
            self.style_error_logged.set(false);
        }
    }

    fn log_style_error(&self, what: &str, value: bool, error: &str) {
        if !self.style_error_logged.replace(true) {
            eprintln!("FLYOUT: could not change the window style ({what}={value}): {error}");
        }
    }

    /// Creates the native window once, off-screen and fully transparent, so
    /// the first real show can start its fade at zero opacity (no flash).
    pub fn warm_up(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        if self.mode.get() != Mode::Hidden || window.window().is_visible() {
            return;
        }
        window
            .window()
            .set_position(slint::PhysicalPosition::new(-32000, -32000));
        self.sync_native_style(&window);
        let _ = window.show();
        install_caption_guard(&window);
        set_alpha(&window, 0);
        let _ = window.hide();
    }

    /// Tray hover is accepted only by the preview controller.
    pub fn hover(self: &Rc<Self>) {
        let Some(decision) =
            hover_decision(self.role, self.mode.get(), self.main_window_is_foreground())
        else {
            return;
        };
        match decision {
            HoverAction::KeepFull => {}
            HoverAction::ShowCompact => {
                self.mode.set(Mode::CompactTemporary);
                self.clear_timers();
                self.present(false);
            }
            HoverAction::KeepCompact => {
                self.hide_deadline.set(None);
                if self.hiding.get() {
                    self.present(false);
                }
            }
        }
    }

    /// Read-only foreground check for deciding whether preview is redundant.
    fn main_window_is_foreground(&self) -> bool {
        let Some(window) = self.main_window.as_ref().and_then(slint::Weak::upgrade) else {
            return false;
        };
        if !window.window().is_visible() {
            return false;
        }
        #[cfg(windows)]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
            let native_window = window.window().window_handle();
            let Ok(handle) = native_window.window_handle() else {
                return false;
            };
            let RawWindowHandle::Win32(handle) = handle.as_raw() else {
                return false;
            };
            unsafe { GetForegroundWindow() == handle.hwnd.get() as _ }
        }
        #[cfg(not(windows))]
        {
            use slint::winit_030::WinitWindowAccessor;
            window
                .window()
                .with_winit_window(|native| native.has_focus())
                .unwrap_or(false)
        }
    }

    /// Pointer movement over the tray icon. The preview opens once the cursor
    /// has rested there for `HOVER_DELAY` (checked in `tick`).
    pub fn pointer_over_icon(&self) {
        if self.role != WindowRole::Preview {
            return;
        }
        let mode = self.mode.get();
        let full_is_foreground = self.main_window_is_foreground();
        let candidate = pointer_hover_candidate(mode, full_is_foreground);
        self.hover_since.set(next_hover_since(
            self.hover_since.get(),
            candidate,
            Instant::now(),
        ));
    }

    /// Double-click or menu "Open": the full flyout, pinned until closed.
    pub fn open_full(self: &Rc<Self>) {
        if self.role != WindowRole::Main {
            return;
        }
        if self.mode.get() == Mode::FullPinned
            && self
                .window
                .upgrade()
                .is_some_and(|window| window.window().is_visible())
        {
            if let Some(window) = self.window.upgrade() {
                activate_window(&window);
            }
            self.clear_timers();
            return;
        }
        self.mode.set(Mode::FullPinned);
        self.clear_timers();
        self.present(true);
    }

    /// Close button: hide to the tray (the application keeps running).
    pub fn close(self: &Rc<Self>) {
        self.remember_pinned_position();
        self.mode.set(Mode::Hidden);
        self.clear_timers();
        self.animate(false, false);
    }

    /// The window was closed by the system (e.g. Alt+F4).
    pub fn closed_externally(&self) {
        self.remember_pinned_position();
        self.mode.set(Mode::Hidden);
        self.clear_timers();
        self.generation.set(self.generation.get() + 1);
        self.animation.stop();
        self.hiding.set(false);
        if let Some(window) = self.window.upgrade() {
            self.sync_native_style(&window);
        }
    }

    /// Menu "Refresh". `request` triggers the real refresh and returns whether
    /// a completion will be reported through `refresh_finished`.
    pub fn menu_refresh(self: &Rc<Self>, request: impl FnOnce() -> bool) {
        let will_complete = request();
        if self.role != WindowRole::Preview
            || self.mode.get() == Mode::FullPinned
            || self
                .main_window
                .as_ref()
                .and_then(slint::Weak::upgrade)
                .is_some_and(|main| main.window().is_visible())
        {
            return;
        }
        if self.mode.get() == Mode::Hidden || self.hiding.get() {
            self.mode.set(Mode::CompactTemporary);
            self.present(false);
        }
        self.hide_deadline.set(None);
        if will_complete {
            self.refresh_started.set(Some(Instant::now()));
            self.hold_until.set(None);
        } else {
            self.refresh_started.set(None);
            self.hold_until.set(Some(Instant::now() + REFRESH_HOLD));
        }
    }

    /// A requested refresh completed: keep the temporary preview 5 more seconds.
    pub fn refresh_finished(&self) {
        if self.refresh_started.take().is_some() && self.mode.get() == Mode::CompactTemporary {
            self.hold_until.set(Some(Instant::now() + REFRESH_HOLD));
        }
    }

    fn clear_timers(&self) {
        self.hide_deadline.set(None);
        self.hold_until.set(None);
        self.refresh_started.set(None);
    }

    /// Lets the user move the pinned flyout by its header.
    pub fn start_drag(&self) {
        if self.role != WindowRole::Main || self.mode.get() != Mode::FullPinned {
            return;
        }
        if let Some(window) = self.window.upgrade() {
            use slint::winit_030::WinitWindowAccessor;
            window.window().with_winit_window(|native| {
                let _ = native.drag_window();
            });
        }
    }

    /// Called every 50 ms on the UI thread: keeps the native rounded shape in
    /// sync and drives the temporary preview's hide deadlines.
    pub fn tick(self: &Rc<Self>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        if window.window().is_visible() {
            install_caption_guard(&window);
            self.apply_native_corners(&window);
        }
        // Safety net for any show/hide path (including system-initiated ones).
        self.sync_native_style(&window);
        let now = Instant::now();
        if let Some(since) = self.hover_since.get() {
            match hover_timer_action(
                self.mode.get(),
                self.main_window_is_foreground(),
                self.cursor_over_icon(),
                now.duration_since(since),
            ) {
                HoverTimerAction::Cancel => self.hover_since.set(None),
                HoverTimerAction::Wait => {}
                HoverTimerAction::Dispatch => {
                    self.hover_since.set(None);
                    self.hover();
                }
            }
        }
        if self.mode.get() != Mode::CompactTemporary || self.hiding.get() {
            return;
        }
        if let Some(started) = self.refresh_started.get() {
            if now.duration_since(started) < REFRESH_TIMEOUT {
                return;
            }
            self.refresh_started.set(None);
            self.hold_until.set(Some(now + REFRESH_HOLD));
        }
        if let Some(hold) = self.hold_until.get() {
            // Refresh hold: only interacting with the preview itself cancels it
            // (the cursor resting on the tray icon after the menu does not).
            if self.cursor_over_window() {
                self.hold_until.set(None);
                self.hide_deadline.set(None);
            } else if now >= hold {
                self.hide_now();
            }
            return;
        }
        if self.cursor_inside() {
            self.hide_deadline.set(None);
            return;
        }
        match self.hide_deadline.get() {
            None => self.hide_deadline.set(Some(now + LEAVE_DELAY)),
            Some(deadline) if now >= deadline => self.hide_now(),
            Some(_) => {}
        }
    }

    /// Hides a temporary compact preview (e.g. when the tray context menu
    /// opens). A pinned full flyout is left alone.
    pub fn dismiss_preview(self: &Rc<Self>) {
        if self.role == WindowRole::Preview && self.mode.get() == Mode::CompactTemporary {
            self.hide_now();
        }
    }

    fn hide_now(self: &Rc<Self>) {
        if self.mode.get() != Mode::CompactTemporary {
            return;
        }
        self.mode.set(Mode::Hidden);
        self.clear_timers();
        self.animate(false, false);
    }

    /// Debug-only review aid: compact preview that stays until closed by timeout.
    #[cfg(debug_assertions)]
    pub fn preview_compact(self: &Rc<Self>) {
        if self.role != WindowRole::Preview {
            return;
        }
        self.hover();
        self.refresh_started.set(Some(Instant::now()));
    }

    fn remember_pinned_position(&self) {
        if self.mode.get() != Mode::FullPinned {
            return;
        }
        if let Some(window) = self.window.upgrade() {
            if window.window().is_visible() {
                self.pinned_position.set(Some(window.window().position()));
            }
        }
    }

    fn physical_size(window: &MainWindow) -> (i32, i32) {
        let scale = window.window().scale_factor();
        (
            (window.get_flyout_width() * scale).round() as i32,
            (window.get_flyout_height() * scale).round() as i32,
        )
    }

    /// Shows this controller's fixed presentation; it never changes role.
    fn present(self: &Rc<Self>, activate: bool) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let visible = window.window().is_visible() && !self.hiding.get();
        let compact = compact_presentation(self.role);
        window.set_compact(compact);
        // Each presentation gets a fresh write budget for activation style.
        self.no_activate_writes.set(0);
        let (width, height) = Self::physical_size(&window);
        let scale = window.window().scale_factor();
        let position = if !compact {
            self.pinned_position
                .get()
                .and_then(|position| restore_position(position, width, height))
                .unwrap_or_else(|| self.anchor(width, height, scale))
        } else {
            self.anchor(width, height, scale)
        };
        self.target.set(Some(Rect {
            x: position.x,
            y: position.y,
            width,
            height,
        }));
        window
            .window()
            .set_size(slint::PhysicalSize::new(width as u32, height as u32));
        self.apply_native_corners(&window);
        if visible {
            // Invalidate any running animation and settle at full opacity.
            self.generation.set(self.generation.get() + 1);
            self.animation.stop();
            set_alpha(&window, 255);
            window.window().set_position(position);
            // The HWND keeps the same role and presentation for its lifetime.
            self.sync_native_style(&window);
            if activate {
                activate_window(&window);
            }
        } else {
            // Install WS_EX_NOACTIVATE before the first ShowWindow call.
            self.sync_native_style(&window);
            self.animate(true, activate);
        }
    }

    /// Fade + 6 px slide: show 180 ms, hide 150 ms. Respects the Windows
    /// "Animation effects" setting (instant when animations are off).
    fn animate(self: &Rc<Self>, showing: bool, activate: bool) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.animation.stop();
        let target = self.target.get().map_or_else(
            || {
                let position = window.window().position();
                slint::PhysicalPosition::new(position.x, position.y)
            },
            |rect| slint::PhysicalPosition::new(rect.x, rect.y),
        );
        if !showing && !window.window().is_visible() {
            self.hiding.set(false);
            return;
        }
        if !animations_enabled() {
            self.hiding.set(false);
            if showing {
                window.window().set_position(target);
                let _ = window.show();
                self.sync_native_style(&window);
                if activate {
                    activate_window(&window);
                }
                set_alpha(&window, 255);
            } else {
                let _ = window.hide();
                self.sync_native_style(&window);
            }
            return;
        }
        let offset = (SLIDE * window.window().scale_factor()).round() as i32;
        let duration = if showing {
            SHOW_DURATION
        } else {
            HIDE_DURATION
        };
        self.hiding.set(!showing);
        if showing {
            set_alpha(&window, 0);
            window
                .window()
                .set_position(slint::PhysicalPosition::new(target.x, target.y + offset));
            let _ = window.show();
            self.sync_native_style(&window);
            if activate {
                activate_window(&window);
            }
            set_alpha(&window, 0);
            self.apply_native_corners(&window);
        }
        let started = Instant::now();
        let this = Rc::downgrade(self);
        self.animation
            .start(slint::TimerMode::Repeated, FRAME, move || {
                let Some(this) = this.upgrade() else {
                    return;
                };
                if this.generation.get() != generation {
                    return;
                }
                let Some(window) = this.window.upgrade() else {
                    return;
                };
                let t = (started.elapsed().as_secs_f32() / duration.as_secs_f32()).min(1.0);
                let eased = 1.0 - (1.0 - t).powi(3);
                let (opacity, shift) = if showing {
                    (eased, (1.0 - eased) * offset as f32)
                } else {
                    (1.0 - eased, eased * offset as f32)
                };
                set_alpha(&window, (opacity * 255.0).round() as u8);
                window.window().set_position(slint::PhysicalPosition::new(
                    target.x,
                    target.y + shift.round() as i32,
                ));
                if t >= 1.0 {
                    this.animation.stop();
                    if !showing {
                        this.hiding.set(false);
                        let _ = window.hide();
                        window.window().set_position(target);
                        this.sync_native_style(&window);
                    }
                }
            });
    }

    fn cursor_inside(&self) -> bool {
        let Some((x, y)) = cursor_position() else {
            return true;
        };
        if (self.icon_rect)().is_some_and(|rect| rect.contains(x, y, 2)) {
            return true;
        }
        self.cursor_over_window()
    }

    fn cursor_over_icon(&self) -> bool {
        let Some((x, y)) = cursor_position() else {
            return false;
        };
        (self.icon_rect)().is_some_and(|rect| rect.contains(x, y, 2))
    }

    fn cursor_over_window(&self) -> bool {
        let Some((x, y)) = cursor_position() else {
            return false;
        };
        self.target.get().is_some_and(|rect| {
            x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
        })
    }

    /// Position near the tray icon inside the work area of its monitor.
    fn anchor(&self, width: i32, height: i32, scale: f32) -> slint::PhysicalPosition {
        let gap = (EDGE_GAP * scale).round() as i32;
        let icon = (self.icon_rect)();
        let (ax, ay) = icon
            .map(|rect| ((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2))
            .or_else(cursor_position)
            .unwrap_or((0, 0));
        let Some(work) = work_area_at(ax, ay) else {
            return slint::PhysicalPosition::new(ax - width / 2, ay - height - gap);
        };
        let clamp_x = |x: i32| x.clamp(work.left + gap, (work.right - width - gap).max(work.left));
        let clamp_y = |y: i32| y.clamp(work.top + gap, (work.bottom - height - gap).max(work.top));
        let (x, y) = if ay >= work.bottom {
            (clamp_x(ax - width / 2), work.bottom - height - gap)
        } else if ay < work.top {
            (clamp_x(ax - width / 2), work.top + gap)
        } else if ax >= work.right {
            (work.right - width - gap, clamp_y(ay - height / 2))
        } else if ax < work.left {
            (work.left + gap, clamp_y(ay - height / 2))
        } else {
            // Icon inside the work area (auto-hide taskbar or overflow area).
            let above = icon.map_or(ay, |rect| rect.top) - height - gap;
            (clamp_x(ax - width / 2), clamp_y(above))
        };
        slint::PhysicalPosition::new(x, y)
    }

    /// Requests antialiased, system-drawn Windows 11 corners and removes the
    /// DWM outline. The Slint surface is opaque and full-bleed (the software
    /// renderer has no per-pixel alpha), so DWM's rounded clip is the only
    /// corner. A GDI window region is avoided: it clips to a jagged binary
    /// pixel boundary.
    fn apply_native_corners(&self, window: &MainWindow) {
        #[cfg(windows)]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            use windows_sys::Win32::Graphics::Dwm::{
                DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
                DwmSetWindowAttribute,
            };
            let handle = window.window().window_handle();
            let Ok(handle) = handle.window_handle() else {
                return;
            };
            let RawWindowHandle::Win32(handle) = handle.as_raw() else {
                return;
            };
            let (width, height) = Self::physical_size(window);
            let key = (handle.hwnd.get(), width, height);
            if width <= 0 || height <= 0 || self.corner_key.get() == Some(key) {
                return;
            }
            let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
            unsafe {
                let preference = DWMWCP_ROUND;
                let result = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                    (&preference as *const i32).cast::<std::ffi::c_void>(),
                    std::mem::size_of::<i32>() as u32,
                );
                if result != 0 {
                    eprintln!(
                        "DWM_CORNER: rounded-corner preference unavailable hr={result:#x}; corners stay square"
                    );
                }
                // No DWM outline (Windows 11 22000+); the DWM shadow stays.
                let border: u32 = DWMWA_COLOR_NONE;
                let result = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_BORDER_COLOR as u32,
                    (&border as *const u32).cast::<std::ffi::c_void>(),
                    std::mem::size_of::<u32>() as u32,
                );
                if result != 0 {
                    eprintln!("DWM_CORNER: border color unavailable hr={result:#x}");
                }
            }
            // Avoid retrying an unsupported DWM attribute on every 50 ms tick.
            self.corner_key.set(Some(key));
        }
        #[cfg(not(windows))]
        let _ = window;
    }
}

/// Moves the flyout into (or out of) the TOPMOST band without activating,
/// moving or resizing it, and without touching the keyboard focus.
#[cfg(windows)]
fn set_topmost(window: &MainWindow, topmost: bool) -> Result<(), String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
        SetWindowPos,
    };
    let slint_handle = window.window().window_handle();
    let handle = slint_handle
        .window_handle()
        .map_err(|error| format!("no native window handle: {error}"))?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err("not a Win32 window".into());
    };
    let insert_after = if topmost {
        HWND_TOPMOST
    } else {
        HWND_NOTOPMOST
    };
    let ok = unsafe {
        SetWindowPos(
            handle.hwnd.get() as _,
            insert_after,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        )
    } != 0;
    if ok {
        Ok(())
    } else {
        Err(format!(
            "SetWindowPos failed with Windows error {}",
            unsafe { GetLastError() }
        ))
    }
}

#[cfg(not(windows))]
fn set_topmost(_window: &MainWindow, _topmost: bool) -> Result<(), String> {
    Ok(())
}

/// Sets or clears `WS_EX_NOACTIVATE` (read-modify-write of the extended
/// style, so `WS_EX_LAYERED` set by `set_alpha` is preserved) and verifies it.
#[cfg(windows)]
fn set_no_activate(window: &MainWindow, no_activate: bool) -> Result<(), String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, SetWindowLongPtrW, WS_EX_NOACTIVATE,
    };
    let slint_handle = window.window().window_handle();
    let handle = slint_handle
        .window_handle()
        .map_err(|error| format!("no native window handle: {error}"))?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err("not a Win32 window".into());
    };
    let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let wanted = if no_activate {
            style | WS_EX_NOACTIVATE
        } else {
            style & !WS_EX_NOACTIVATE
        };
        if wanted != style {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted as isize);
        }
        let applied = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_NOACTIVATE != 0;
        if applied == no_activate {
            Ok(())
        } else {
            Err("extended style did not change".into())
        }
    }
}

#[cfg(not(windows))]
fn set_no_activate(_window: &MainWindow, _no_activate: bool) -> Result<(), String> {
    Ok(())
}

/// Live `WS_EX_NOACTIVATE` state of the native window, if it exists.
#[cfg(windows)]
fn read_no_activate(window: &MainWindow) -> Option<bool> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, WS_EX_NOACTIVATE,
    };
    let slint_handle = window.window().window_handle();
    let handle = slint_handle.window_handle().ok()?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return None;
    };
    let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    Some(style & WS_EX_NOACTIVATE != 0)
}

#[cfg(not(windows))]
fn read_no_activate(_window: &MainWindow) -> Option<bool> {
    None
}

/// Explicit tray activation brings the flyout forward using ordinary Windows
/// foreground rules; it does not establish a topmost Z-order band.
fn activate_window(window: &MainWindow) {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
        let handle = window.window().window_handle();
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
    let _ = window;
}

#[cfg(not(windows))]
fn activate_window(window: &MainWindow) {
    use slint::winit_030::{WinitWindowAccessor, winit::window::WindowLevel};
    window.window().with_winit_window(|native| {
        native.set_window_level(WindowLevel::Normal);
        native.focus_window();
    });
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WorkArea {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

#[cfg(windows)]
pub(crate) fn work_area_at(x: i32, y: i32) -> Option<WorkArea> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    };
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        if monitor.is_null() {
            return None;
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        Some(WorkArea {
            left: info.rcWork.left,
            top: info.rcWork.top,
            right: info.rcWork.right,
            bottom: info.rcWork.bottom,
        })
    }
}

#[cfg(not(windows))]
pub(crate) fn work_area_at(_x: i32, _y: i32) -> Option<WorkArea> {
    None
}

/// Restores a remembered pinned position unchanged while its center remains on
/// a monitor. If that monitor disappeared, the caller falls back to the tray.
fn restore_position(
    position: slint::PhysicalPosition,
    width: i32,
    height: i32,
) -> Option<slint::PhysicalPosition> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromPoint};
        let center = POINT {
            x: position.x + width / 2,
            y: position.y + height / 2,
        };
        let center_on_monitor =
            !unsafe { MonitorFromPoint(center, MONITOR_DEFAULTTONULL) }.is_null();
        restore_pinned_position(position, center_on_monitor)
    }
    #[cfg(not(windows))]
    {
        restore_pinned_position(
            position,
            work_area_at(position.x + width / 2, position.y + height / 2).is_some(),
        )
    }
}

fn restore_pinned_position(
    position: slint::PhysicalPosition,
    center_on_monitor: bool,
) -> Option<slint::PhysicalPosition> {
    center_on_monitor.then_some(position)
}

#[cfg(windows)]
fn cursor_position() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut point = POINT { x: 0, y: 0 };
    (unsafe { GetCursorPos(&mut point) } != 0).then_some((point.x, point.y))
}

#[cfg(not(windows))]
fn cursor_position() -> Option<(i32, i32)> {
    None
}

/// Windows "Animation effects" (reduced motion when off).
#[cfg(windows)]
fn animations_enabled() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
    };
    let mut enabled: i32 = 1;
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut enabled as *mut i32).cast::<std::ffi::c_void>(),
            0,
        )
    };
    ok == 0 || enabled != 0
}

#[cfg(not(windows))]
fn animations_enabled() -> bool {
    false
}

/// Prevents the white native (tool-window) caption from being painted over the
/// frameless flyout on click, drag or focus change. winit keeps
/// `WS_CAPTION | WS_SYSMENU` on undecorated windows (hiding the caption only via
/// `WM_NCCALCSIZE`) and forwards `WM_NCACTIVATE` to `DefWindowProc` unchanged,
/// so Windows still draws the caption, its close button and frame line.
///
/// 1. Remove the caption/system-menu style bits (dragging uses `drag_window`,
///    which does not need a caption). winit may restore them on show/hide, so
///    this runs after the first show and on every tick.
/// 2. Subclass the window: `WM_NCACTIVATE` goes on with `lParam = -1` (activation
///    state still changes, winit still sees it, no non-client repaint) and
///    `WM_NCPAINT` is swallowed (the flyout has no non-client area).
///    Installing the same proc/id again only updates it, so repeats are safe.
fn install_caption_guard(window: &MainWindow) {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let handle = window.window().window_handle();
        let Ok(handle) = handle.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
        guard_caption_hwnd(hwnd);
    }
    #[cfg(not(windows))]
    let _ = window;
}

/// Same guard for any frameless AUM window (flyout, tray menu), by handle.
#[cfg(windows)]
pub(crate) fn guard_caption_hwnd(hwnd: windows_sys::Win32::Foundation::HWND) {
    {
        use windows_sys::Win32::UI::Shell::SetWindowSubclass;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GWL_STYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_CAPTION, WS_MAXIMIZEBOX,
            WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
        };
        let caption = WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;
        unsafe {
            SetWindowSubclass(hwnd, Some(caption_guard_proc), CAPTION_GUARD_ID, 0);
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            if style & caption != 0 {
                SetWindowLongPtrW(hwnd, GWL_STYLE, (style & !caption) as isize);
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
    }
}

#[cfg(windows)]
const CAPTION_GUARD_ID: usize = 0x4155_4d31; // "AUM1"

#[cfg(windows)]
unsafe extern "system" fn caption_guard_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::Shell::DefSubclassProc;
    use windows_sys::Win32::UI::WindowsAndMessaging::{WM_NCACTIVATE, WM_NCPAINT};
    match message {
        WM_NCPAINT => 0,
        WM_NCACTIVATE => unsafe { DefSubclassProc(hwnd, message, wparam, -1) },
        _ => unsafe { DefSubclassProc(hwnd, message, wparam, lparam) },
    }
}

/// Whole-window opacity through a layered window (used for the fade).
fn set_alpha(window: &MainWindow, alpha: u8) {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongPtrW, LWA_ALPHA, SetLayeredWindowAttributes,
            SetWindowLongPtrW, WS_EX_LAYERED,
        };
        let handle = window.window().window_handle();
        let Ok(handle) = handle.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            if style & WS_EX_LAYERED == 0 {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (style | WS_EX_LAYERED) as isize);
            }
            let _ = SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA);
        }
    }
    #[cfg(not(windows))]
    let _ = (window, alpha);
}

#[cfg(test)]
mod tests {
    use super::{
        HOVER_DELAY, HoverAction, HoverTimerAction, MAX_NO_ACTIVATE_WRITES, Mode, NativeStyle,
        StyleAction, WindowRole, compact_presentation, hover_action, hover_decision,
        hover_timer_action, native_style, next_hover_since, no_activate_action,
        pointer_hover_candidate, restore_pinned_position,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn no_activate_is_decided_from_the_live_style() {
        // Matching live style: nothing to write, whatever the budget.
        for writes in [0, MAX_NO_ACTIVATE_WRITES, MAX_NO_ACTIVATE_WRITES + 1] {
            assert_eq!(no_activate_action(true, true, writes), StyleAction::Keep);
            assert_eq!(no_activate_action(false, false, writes), StyleAction::Keep);
        }
        // Live style differs (e.g. winit dropped it after ShowWindow): write,
        // even though AUM's own last write was the wanted value.
        assert_eq!(no_activate_action(false, true, 0), StyleAction::Apply);
        assert_eq!(no_activate_action(true, false, 1), StyleAction::Apply);
        // Budget exhausted: stop instead of writing on every tick.
        assert_eq!(
            no_activate_action(false, true, MAX_NO_ACTIVATE_WRITES),
            StyleAction::GiveUp
        );
    }

    /// Simulates one compact show with winit's post-show and post-hide style
    /// rewrites: the style is restored each time and stays within budget.
    #[test]
    fn winit_style_rewrites_are_repaired_within_the_write_budget() {
        let mut live = false;
        let mut writes = 0;
        let sync =
            |live: &mut bool, writes: &mut u32| match no_activate_action(*live, true, *writes) {
                StyleAction::Apply => {
                    *writes += 1;
                    *live = true;
                }
                StyleAction::Keep => {}
                StyleAction::GiveUp => panic!("budget exhausted in a normal show/hide"),
            };
        sync(&mut live, &mut writes); // present(): before the show
        assert!(live);
        live = false; // winit: ShowWindow + GWL_EXSTYLE rewrite
        sync(&mut live, &mut writes); // post-show sync / tick
        assert!(live);
        sync(&mut live, &mut writes); // later ticks: no further writes
        sync(&mut live, &mut writes);
        live = false; // winit: hide + rewrite
        sync(&mut live, &mut writes);
        assert!(live);
        assert_eq!(writes, 3);
    }

    #[test]
    fn a_style_that_keeps_being_reset_stops_after_the_budget() {
        let mut writes = 0;
        let mut applied = 0;
        for _tick in 0..100 {
            // Something resets the style before every sync.
            match no_activate_action(false, true, writes) {
                StyleAction::Apply => {
                    writes += 1;
                    applied += 1;
                }
                StyleAction::GiveUp => {}
                StyleAction::Keep => unreachable!(),
            }
        }
        assert_eq!(applied, MAX_NO_ACTIVATE_WRITES);
    }

    fn style(topmost: bool, no_activate: bool) -> NativeStyle {
        NativeStyle {
            topmost,
            no_activate,
        }
    }

    #[test]
    fn only_the_visible_compact_preview_is_topmost() {
        assert!(native_style(WindowRole::Preview, true).topmost);
        assert!(!native_style(WindowRole::Preview, false).topmost);
        assert!(!native_style(WindowRole::Main, true).topmost);
        assert!(!native_style(WindowRole::Main, false).topmost);
    }

    #[test]
    fn compact_preview_never_activates_and_full_flyout_always_can() {
        // Compact: non-activatable before it is shown (style must precede
        // winit's SW_SHOW), while shown, and while fading out.
        assert!(native_style(WindowRole::Preview, false).no_activate);
        assert!(native_style(WindowRole::Preview, true).no_activate);
        assert!(!native_style(WindowRole::Main, false).no_activate);
        assert!(!native_style(WindowRole::Main, true).no_activate);
    }

    #[test]
    fn tray_hover_decision_distinguishes_closed_active_and_inactive_full_window() {
        assert_eq!(hover_action(Mode::Hidden, false), HoverAction::ShowCompact);
        assert_eq!(
            hover_action(Mode::Hidden, true),
            HoverAction::KeepFull,
            "an active main window must not get a redundant preview"
        );
        assert_eq!(
            hover_action(Mode::CompactTemporary, false),
            HoverAction::KeepCompact
        );
        assert_eq!(
            hover_action(Mode::CompactTemporary, true),
            HoverAction::KeepCompact
        );
        assert_eq!(hover_action(Mode::FullPinned, true), HoverAction::KeepFull);
        assert_eq!(
            hover_action(Mode::FullPinned, false),
            HoverAction::ShowCompact
        );
    }

    #[test]
    fn pointer_hover_scheduler_dispatches_for_inactive_full_and_cancels_for_active_full() {
        let start = Instant::now();
        assert!(pointer_hover_candidate(Mode::Hidden, false));
        assert!(!pointer_hover_candidate(Mode::Hidden, true));
        assert!(pointer_hover_candidate(Mode::FullPinned, false));
        assert!(!pointer_hover_candidate(Mode::FullPinned, true));
        assert!(!pointer_hover_candidate(Mode::CompactTemporary, false));

        let pending = next_hover_since(None, true, start).expect("candidate starts timer");
        assert_eq!(pending, start);
        assert_eq!(
            next_hover_since(Some(pending), true, start + Duration::from_millis(100)),
            Some(start),
            "repeated pointer events must not restart the hover delay"
        );
        assert_eq!(
            hover_timer_action(
                Mode::FullPinned,
                false,
                true,
                HOVER_DELAY - Duration::from_millis(1)
            ),
            HoverTimerAction::Wait
        );
        assert_eq!(
            hover_timer_action(Mode::FullPinned, false, true, HOVER_DELAY),
            HoverTimerAction::Dispatch
        );
        assert_eq!(
            hover_timer_action(Mode::FullPinned, true, true, HOVER_DELAY),
            HoverTimerAction::Cancel,
            "foreground full window must cancel pending compact dispatch"
        );
        assert_eq!(
            hover_timer_action(Mode::FullPinned, false, false, HOVER_DELAY),
            HoverTimerAction::Cancel,
            "fast pointer departure must cancel pending dispatch"
        );
    }

    #[test]
    fn main_and_preview_lifecycles_are_independent() {
        let main = Mode::FullPinned;
        let mut preview = Mode::Hidden;

        // Hover while the main is active leaves it untouched and suppresses preview.
        assert_eq!(hover_action(preview, true), HoverAction::KeepFull);
        assert_eq!(main, Mode::FullPinned);
        assert_eq!(preview, Mode::Hidden);

        // Background hover changes only the reusable preview's lifecycle.
        assert_eq!(hover_action(preview, false), HoverAction::ShowCompact);
        preview = Mode::CompactTemporary;
        assert_eq!(preview, Mode::CompactTemporary);
        assert_eq!(main, Mode::FullPinned);
        assert!(!native_style(WindowRole::Main, true).topmost);
        assert!(native_style(WindowRole::Preview, true).topmost);
        assert!(native_style(WindowRole::Preview, true).no_activate);

        // Dismissal hides only the preview; main remains visible and pinned.
        preview = Mode::Hidden;
        assert_eq!(main, Mode::FullPinned);
        assert_eq!(preview, Mode::Hidden);

        // Explicit Open reuses/activates the existing main state.
        assert_eq!(main, Mode::FullPinned);
        assert!(!native_style(WindowRole::Main, true).topmost);
    }

    #[test]
    fn tray_hover_is_owned_only_by_the_preview_controller() {
        assert!(!compact_presentation(WindowRole::Main));
        assert!(compact_presentation(WindowRole::Preview));
        assert_eq!(
            hover_decision(WindowRole::Main, Mode::FullPinned, false),
            None,
            "tray hover must never dispatch a presentation change to main"
        );
        assert_eq!(
            hover_decision(WindowRole::Preview, Mode::Hidden, false),
            Some(HoverAction::ShowCompact)
        );
        assert_eq!(
            hover_decision(WindowRole::Preview, Mode::Hidden, true),
            Some(HoverAction::KeepFull),
            "active main suppresses a redundant compact preview"
        );
    }

    #[test]
    fn native_styles_are_fixed_by_independent_window_role() {
        // Each HWND keeps one role for its entire lifetime, including show,
        // fade, dismissal, hide and reopen.
        let steps = [
            ((WindowRole::Main, false), style(false, false)),
            ((WindowRole::Main, true), style(false, false)),
            ((WindowRole::Preview, false), style(false, true)),
            ((WindowRole::Preview, true), style(true, true)),
        ];
        for ((role, visible), expected) in steps {
            assert_eq!(
                native_style(role, visible),
                expected,
                "{role:?} visible={visible}"
            );
        }
    }

    #[test]
    fn saved_position_is_preserved_while_its_monitor_exists() {
        let saved = slint::PhysicalPosition::new(2, 3);
        assert_eq!(
            restore_pinned_position(saved, true),
            Some(saved),
            "a pinned position on a connected monitor must not be clamped or re-anchored"
        );
        assert_eq!(
            restore_pinned_position(saved, false),
            None,
            "when the monitor containing the saved center disappears, use the tray anchor"
        );
    }
}
