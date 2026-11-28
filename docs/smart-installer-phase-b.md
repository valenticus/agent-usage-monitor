# Smart Installer Phase B

The Inno Setup wizard keeps its existing per-user identity, install path,
license file, process guard, migration logic, and standard file installation.
The welcome, ready, and completion pages are hidden. The license page remains
visible and requires explicit acceptance; after acceptance, Setup advances
asynchronously through the normal wizard button event so directory validation
and `PrepareToInstall` still run. A localized Retry control remains available
if preparation reports an error. The standard Cancel action stays available.

Inno's own Installing page provides the installation status and real extraction
progress. No custom file-copy loop or progress timer is used. After a successful
interactive install, `ssDone` starts AUM once and Setup closes. Silent installs
do not launch AUM. The old `[Run] postinstall` launch entry is removed because
that entry depends on completion-page behavior.

The wizard may require a user interaction to accept the license. This is
intentional: Setup does not silently accept the license to achieve automatic
progress. If the asynchronous transition cannot be scheduled, the installer
shows a localized message and exposes a manual Install now fallback.

Compilation and source-level regression tests do not verify native layout,
keyboard accessibility, cancellation timing, upgrade callbacks, or license
rendering. Those still require an isolated Windows QA run; never test by
installing over the user's active AUM release candidate.
