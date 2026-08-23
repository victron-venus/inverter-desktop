# Changelog

All notable changes to this project will be documented in this file.

## [2.4.4] - 2026-08-23

### Added
- Solar forecast display (#252) and forecast brackets in DailyStats (#251)
- Yesterday solar production (#241)
- Washer/dryer START/PAUSE buttons from HA button entities (#247)
- Persistent notification banner + Victron alarm watcher (#244)

### Fixed
- Tray icon keeps updating after poisoned lock, panic, or sleep/wake (#255)
- Solar breakdown now adds up to headline total (#254)
- Existing config/about windows brought to front on reopen (#253)

### Security
- Bump glob override to 11.1.0 (CVE-2025-64756) (#250)

## [2.4.3] - unreleased

### Fixed
- Various fixes and dependency updates

## [2.2.2] - 2026-07-20

### Fixed

- CI build failure: remove empty Apple code signing env vars causing `SecKeychainItemImport` error
