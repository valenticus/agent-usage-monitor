# Smart Installer Phase B

The installer requires Inno Setup 6.7.3 or newer. A preprocessor version check
in `AgentUsageMonitor.iss` rejects older compilers. The stable AppId, per-user
scope, install identity, migration behavior, process guard, license file and
standard `[Files]` installation remain in place.

The welcome, destination, ready and completion pages are hidden. Inno's
`UsePreviousAppDir=yes` keeps the recorded directory for an existing install.
For a fresh install with a custom location, pass the standard Inno `/DIR`
parameter, for example:

```text
AgentUsageMonitor-0.3.0-win-x64-setup.exe /DIR="D:\Apps\AgentUsageMonitor"
```

The AUM pre-install checks still reject an existing install at a mismatched
path and refuse to overwrite an unrelated or unverified AUM executable,
including a Portable copy at the requested path. To change a rejected path,
cancel Setup and relaunch it with the correct `/DIR`. Portable copies at other
locations are untouched.

The license page remains visible and requires explicit acceptance. Its radio
control can be reached and selected using the keyboard; the automatic install
transition begins after acceptance. Inno's normal page actions perform all
validation and invoke `PrepareToInstall` before copying files.

If `PrepareToInstall` returns an error, Inno 6.7.3 remains on the Preparing
page, displays the error, disables its Next control and focuses Cancel. The
installer adds a separate keyboard-accessible Retry control from the shared
failure callback, so it appears whether the failed attempt followed automatic
advancement or a manual Install action. Retry navigates Back through Inno's
normal page action; the automatic transition then re-enters the normal Next
path and calls `PrepareToInstall` again. Destination errors explain that Setup
must be canceled and relaunched with the correct `/DIR` parameter.

Inno retains its native controls on Preparing and Finished pages. This keeps
the standard restart-required preflight action available; the installer does
not hide its Finish/restart control. An explicit `InstallCompleted` guard is
set only at `ssPostInstall`, so a preflight restart, failure or cancellation
cannot launch AUM at `ssDone`.

Automatic page advancement uses a one-shot timer and sends `BM_CLICK` only to
Inno's visible, enabled Next button. It checks the page ID immediately after
the normal VCL event returns; an ineffective click exposes a localized manual
action. It never sends the message to a hidden button or schedules repeated
automatic clicks. The standard Cancel action remains available.

The standard Installing page shows Inno's actual file progress. README.md stays
packaged and installed, without the `isreadme` flag that would offer to open it.
After successful interactive setup, `ssDone` launches AUM once with
`ExecAsOriginalUser`; silent setup, failure, cancellation, restart-required
preflight and uninstall do not launch it. If Setup is explicitly started with **Run as administrator**, Inno
cannot always recover the original unelevated token; normal per-user launch
does not require elevation.

Compilation and source-level tests do not verify native layout, Retry keyboard
focus, cancellation timing, path migration, launch token behavior or screen
reader output. These require an isolated Windows QA run; do not test by
installing over the user's active AUM release candidate.
