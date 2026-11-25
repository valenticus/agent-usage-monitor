# Settings window sizing

The Settings window keeps General, Notifications, and About instantiated as
separate Slint layouts at the same scroll origin. It measures each layout's
natural preferred height and uses the maximum as the common opening height.
Inactive pages remain measurable. The active page alone determines the
Flickable's scrollable content height, and selecting a tab resets its scroll
offset to the top. The About footer uses its natural content height instead of
stretching to fill the viewport.

Before the initial measurement, the hidden window is assigned its intended
logical width (720 px). Slint then gets one event-loop turn to lay out wrapped
text at that width. This matters because an unshown Winit window can report
zero width; measuring wrapped text at zero width makes natural heights
artificially large. Only after that layout turn does the app read all three
page preferred heights, choose their maximum, apply the work-area cap, and
show the window at the final size. Thus the work area is an upper bound, not
the desired height. Manual native resizing remains available. Tab changes,
settings changes, language changes, and live status updates do not resize the
open window. A language change takes effect for the next opening measurement.
The About page uses one shared layout for Portable, Installer, and Store. The
status area has no height when empty; when text is present it uses the wrapped
text's preferred height (24–72 logical px) and can add content to the active
page's scrollable area. An 8 px gap separates a present status from the action
row. Status changes never resize the open window. The Store variant shows its
localized Microsoft Store update notice in that same area and hides both
GitHub update controls; it does not reserve a placeholder button row.

On Windows, the opening size is capped to the active monitor's `rcWork` area,
which excludes the taskbar. The calculation accounts for the native caption, a
small screen margin, and the monitor's effective DPI. The minimum height is
reduced when necessary, leaving the existing content Flickable to scroll.
After showing, a one-shot check corrects first-open placement using the
window's actual monitor and current size. It does not resize a fitting window.

While visible, the window checks for monitor, DPI, or work-area changes. A
user-resized width and height are kept if they fit the new work area; only
dimensions that exceed the new bounds are clamped. The position is kept unless
it must be clamped to remain on-screen. Monitor-bound adjustment is deferred
while the left mouse button is held, covering native dragging and edge resize.
The existing 720 logical pixel default width and 680 pixel minimum width are
retained, subject to work-area constraints.

This phase does not include physical multi-monitor or DPI testing. Unit tests
exercise work-area geometry at 100%, 125%, 150%, and 200% scale, including a
taskbar-reduced work area and a monitor with negative horizontal coordinates.
The UI test uses the Slint 1.19 testing backend to measure all three actual
Settings pages, confirms they have distinct preferred heights, and verifies
the shared maximum remains the same while tabs and update states change. With
a constrained viewport, it scrolls About to the bottom, switches to General,
and checks that scrolling resets and the active page's content height shrinks.
It also checks preservation of a fitting manual size and clamping of an
oversized size against simulated DPI/work-area changes.

The 1080p regression covers all four supported locales. At the intended width,
the current normal-content measurements are General 641 px, Notifications
606–623 px, and About 726 px with no update status. The About page becomes
750 px with a normal status and 778 px with a long status. The test also
reproduces the pre-fix zero-width condition, which measured a 1,800–1,976 px
maximum and would be capped near the full work-area height. In an isolated
Windows QA run at 1920×1080/96 DPI, the final build measured 736×765 physical
pixels with no Portable status, 736×789 with an Installer offline status, and
736×789 with the Store notice (native frame included). The former oversized
window measured 736×1,007 px. The physical check covers one monitor and DPI
only; multi-monitor and alternate-DPI behavior still needs manual QA.

The About regression exercises all 3 distributions × 4 locales × 6 states
(empty, available, long available, offline, HTTP error, and invalid data) with
the Slint 1.19 test backend. At normal width, Portable and Installer measure
726 px with no status, 750 px with a normal/error status, and 761 px (Ukrainian)
or 778 px (English, Polish, Russian) with the localized long status. Store
measures 750 px because its localized Store notice is always present. The
Store checks also assert that
the GitHub check and release controls are hidden. The test holds native window
size stable while changing status and selecting all three tabs.

For isolated visual QA, a debug build accepts
`--qa-about-distribution=portable|installer|store` and
`--qa-about-status=available|long|offline|http|invalid`; omit status for the
empty state. These arguments are honored only together with `--qa-isolated
--empty`. They replace display-only About values; the runtime package
detector, update worker, and persisted settings are not changed. Production
builds do not include this preview mechanism.
