# Settings window sizing

The Settings window keeps General, Notifications, and About instantiated as
separate Slint layouts at the same scroll origin. It measures each layout's
natural preferred height and uses one shared opening height. Inactive pages
remain measurable. The active page alone determines the Flickable's scrollable
content height, and selecting a tab resets its scroll offset to the top. All
three tabs use the same native window dimensions.

Before the initial measurement, the hidden window is assigned its intended
logical width (720 px). Slint then gets one event-loop turn to lay out wrapped
text at that width. This matters because an unshown Winit window can report
zero width; measuring wrapped text at zero width makes natural heights
artificially large. Only after that layout turn does the app read all three
page preferred heights, choose their maximum, apply the work-area cap, and
show the window at the final size. Thus the work area is an upper bound, not
the desired height. Manual native resizing remains available and is retained
in memory when Settings closes; reopening respects that size when it fits the
work area, otherwise the existing work-area clamp applies. Because native
font/text metrics can settle after the hidden-window measurement, Windows
performs one additional common-page measurement 150 ms after first show. It
applies that measured height only if no size had been retained and the user has
not resized the window in the meantime. The same work-area clamp is applied.
"Not resized" means the current size is still one the app owns: the size the
first fit requested, or the pre-fit size if the native resize has not landed
yet. Winit applies `set_size` asynchronously on Windows, so `Window::size()`
read right after the first fit is stale and must not be used as the reference;
doing so misread the app's own resize as a manual one and skipped the fit.
Reopens with a retained manual size keep that size. Tab changes, settings
changes, language changes, and live status updates do not resize the open
window. The About page uses one shared layout for Portable, Installer,
and Store. The idle status row collapses completely, retaining the 12 logical
px gap before the update controls. A present status uses its natural wrapped
height and the existing 8 px trailing gap.

The previous sizing calculation subtracted the full dynamic status row and its
8 px gap from the common height. This kept the native size fixed but left a
26 px scroll extent for normal one-line results. The fix reclaims that measured
26 px from four existing post-action gaps: 16/16/16/10 px become 8/8/8/8 px
while a status is visible. Those gaps return to their original values when the
status is idle, so the UI reserves no empty status slot and does not increase
the Settings minimum height. The common-height calculation removes only status
growth left after this existing spacing has been reclaimed. Ordinary localized
messages therefore fit with zero scroll; longer wrapped text keeps its full
natural height and uses the existing Flickable for the excess. The update
button's position remains fixed. The Store variant shows its localized
Microsoft Store notice in the same status element and hides both GitHub
controls.

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

The Slint 1.19 test backend now records window size, viewport/content heights,
page preferred height, shared required height, page padding, status/button/footer
positions, status natural height, maximum scroll extent, and content-y before
and after a bottom-scroll attempt. At 720 logical px width, the headless window
is 720×738 px for all states. General measures 641 px; Notifications measures
606 px in Ukrainian, English, and Polish and 623 px in Russian; About measures
738 px both idle and with each ordinary result. All three tabs have zero
scroll extent and content-y remains 0.0. In English, the update button stays
at y=465 px, the status row at y=505 px, and the copyright bottom at y=714 px;
the ordinary status height is 18 px. Idle status height is 0 px.

Before the fix, the same English test geometry was 720×738 px idle and 720×738
px during a normal result, while About preferred height grew to 764 px. Its
26 px scroll extent moved content-y from 0 to -26 px. The button's document
position remained y=465 px; the visible shift came from scrolling the entire
page to reveal the footer. After the fix, ordinary states remain at 720×738 px
with zero scroll and the footer remains at y=714 px. The synthetic long message
measures 52 px high; after reclaiming 26 px from the existing gaps, its page is
772 px with a real 34 px scroll extent. Scrolling to the bottom reveals the
footer without clipping the status text.

These dimensions are headless Slint logical measurements. They do not include
or report native Windows frame dimensions, and they are not a visual pass.
Work-area geometry has unit coverage at 100%, 125%, 150%, and 200% scale,
including a taskbar-reduced work area and a monitor with negative horizontal
coordinates. The current change still needs manual Windows GUI review, including
actual work-area clamping and 150% DPI where available.

The isolated debug preview accepts `--qa-geometry` with
`--qa-isolated --empty`. Every 500 ms while Settings is visible it reports the
native Win32 outer and client rectangle sizes in physical pixels, the Slint
window size converted to logical pixels using the active scale factor, the
Flickable viewport/content/scroll extent/content-y, active page preferred
and shared required heights, About component heights/positions, dynamic status
height/reclaimed spacing, and update-button/footer positions. It reports the
first-show eligibility inputs, screen lookup failure, requested and work-area
clamped size, whether `set_size` was called, and the immediate result. Before
and after the first-show native fit it emits a labeled geometry sample; later
500 ms samples show the settled native client size. This
distinguishes native frame/client sizing from Slint's logical layout and makes
it possible to correlate an attempted user scroll with the actual extent.
These measurements must be collected from the affected Windows display; the
headless backend does not reproduce the reported native overflow.

The status regression exercises Portable, Installer, and Store layouts across
all four locales, all three themes, repeated Idle/Checking/Current/Available/
Offline/HTTP/Invalid/Timeout transitions, and long text. It asserts identical
window size on every tab, zero scroll for normal messages, a stable update
button and footer, readable natural status height, and real scrolling for long
text. A 420×360 logical viewport test confirms that constrained content scrolls
to reveal the footer. The Store checks confirm the GitHub controls stay hidden.

For isolated visual QA, a debug build accepts
`--qa-about-distribution=portable|installer|store` and
`--qa-about-status=idle|checking|current|available|long|offline|http|invalid|timeout`.
These arguments are honored only together with `--qa-isolated --empty`. They
replace display-only About values; the runtime package
detector, update worker, and persisted settings are not changed. Production
builds do not include this preview mechanism.
