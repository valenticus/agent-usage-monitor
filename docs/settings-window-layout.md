# Settings window sizing

The Settings window keeps General, Notifications, and About instantiated as
separate Slint layouts at the same scroll origin. It measures each layout's
natural preferred height and uses the maximum as the common opening height.
Inactive pages remain measurable. The active page alone determines the
Flickable's scrollable content height, and selecting a tab resets its scroll
offset to the top. The About footer uses its natural content height instead of
stretching to fill the viewport.

The initial size is calculated before showing the window to avoid an opening
resize flash. Manual native resizing remains available. Tab changes, settings
changes, language changes, and live status updates do not resize the open
window. A language change takes effect for the next opening measurement. The
update status area and its action row reserve space for an available release.
Dynamic provider text can increase the Flickable's scrollable content while
the window remains at its current size.

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
