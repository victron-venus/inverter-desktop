# Application release identity

The repository commits a base version in the npm, Cargo, Cargo lock, and Tauri files
declared in `.release-policy.json`. The shared toolkit prepares base changes in a PR.
Release builds download one frozen `.release-plan.json` and apply its version inputs
before compiling any platform. The resulting candidate changes are build overlays;
the source tag alone does not contain those rewritten manifest values.

This application uses `final-build`: beta, RC, and stable have their own embedded
identity. Stable is rebuilt from the accepted RC source and requires acceptance of
the new packages. It does not reuse the RC binaries unchanged.

`get_release_info` returns the descriptor validated and embedded by `build.rs`. Both
the main status bar and About display its complete version while offline. A local
build without a plan displays the base and records channel `local`; a stale,
inconsistent, or missing candidate plan stops compilation.

Native representations are separate: Tauri and Apple marketing metadata keep the
base; Apple build versions use the toolkit's numeric projection. Android reads the
plan directly for its full `versionName` and reserved `versionCode`, because Tauri
regenerates `tauri.properties`. The initial counter floor is 2005042, verified from
the existing 2.5.42 APK. Counters across Play tracks must remain above published
values when migrating or restoring release history.

The iOS project initializer regenerates its plist. That generated file is therefore
outside the declared source-input receipt; `ios_version.py` applies and checks the
plan after initialization. Uploads verify Apple plist, APK/AAB, Linux package, and
Windows installer metadata. Apple, Android, and Linux checks also inspect the
compiled version/source descriptor; package hashes are then bound to the input
receipt. The shared publication gate checks those receipts before publication.

Run `python3 scripts/version_plan.py check-base` for committed source consistency.
Use `pnpm tauri build` for ordinary local builds. To reproduce a release, use a
disposable checkout at the plan's source SHA, download its saved plan, then run
`scripts/release-build.sh X.Y.Z CHANNEL`. This rewrites declared version files.
Hosted installer/store and physical-device acceptance remain separate checks.
