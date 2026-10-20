# RustSec retirement notices and the zbus 6 migration

The Cargo Deny failure observed on 18 October 2026 is caused by three advisories
published on 16 October 2026. They are
**informational `unmaintained` advisories**, not by three newly disclosed
vulnerabilities:

- [RUSTSEC-2026-0370](https://rustsec.org/advisories/RUSTSEC-2026-0370.html): `zvariant`
- [RUSTSEC-2026-0371](https://rustsec.org/advisories/RUSTSEC-2026-0371.html): `zbus_names`
- [RUSTSEC-2026-0372](https://rustsec.org/advisories/RUSTSEC-2026-0372.html): `zvariant_derive`

These standalone crates were merged into zbus 6. Their retirement is described in
[the upstream release checklist](https://github.com/z-galaxy/zbus/issues/2043).
This does not imply that zbus 5 will receive future security updates: upstream's
[security policy](https://github.com/z-galaxy/zbus/blob/main/SECURITY.md) promises
updates for the latest release.

## Temporary decision

The three retirement notices are deferred **until 17 November 2026 at 00:00 UTC**.
The exception does not cover vulnerability advisories. No production dependency,
keyring backend, credential format, notification backend or Tauri version changes
as part of this decision.

The currently published stable versions of `notify-rust`, Tauri's opener plugin,
and the Secret Service keyring stack still use zbus 5. The
[Tauri maintainer's recommendation](https://github.com/tauri-apps/plugins-workspace/issues/3716#issuecomment-6242183511)
is to coordinate the migration after initial zbus 6 patch releases.
[notify-rust's migration PR](https://github.com/hoodie/notify-rust/pull/294) is
not yet a released upgrade path.

Locally forking the four dependency stacks would require a separate runtime
migration: zbus 6 removes its blocking API, changes runtimes and changes wire
types. Switching keyring implementations solely to remove a warning would also
need proof that existing encrypted configurations can still retrieve their key
from the same Secret Service item. Keeping the current tested runtime while
tracking that upstream work avoids introducing this unrelated compatibility
change into a documentation update.

### Dependency review on 20 October 2026

The notification plugin is updated from 2.6.0 to 2.7.0 and the opener plugin
from 2.7.1 to 2.7.2 together with their JavaScript packages. The
[notification release](https://github.com/tauri-apps/plugins-workspace/commit/4356356dab3b87faffcc2f0fc62aec23aede78cc)
keeps its Linux D-Bus connection open, and the
[opener release](https://github.com/tauri-apps/plugins-workspace/commit/1ab930465f43fc4574ce71ed2bda57b1f80a802b)
moves blocking file-reveal calls to a dedicated thread. Both still use zbus 5.

The reviewed Linux dependency graph retains `zbus` 5.16.0, `notify-rust` 4.18.2,
`secret-service` 5.1.0, `zbus-secret-service-keyring-store` 1.0.0 and `keyring`
4.2.0. The three fresh RustSec records still classify the notices as
`unmaintained` and list no patched versions. Only the two plugin versions in
the guard's reviewed inventory change; the 17 November deadline, advisory
classification checks, exact dependency checks and exception list stay intact.

Future JavaScript and Rust plugin updates must be reviewed together. A changed
dependency must trigger this review again; do not remove the version guard or
extend the deadline to accommodate an automated dependency PR.

## Enforcement

[`scripts/check_rustsec_deferrals.py`](../scripts/check_rustsec_deferrals.py) is the
single source of the deadline and exact reviewed dependency versions. Cargo Deny
CI fetches the current RustSec database, runs this guard, and then runs all four
normal checks against the same offline snapshot. The guard fails if:

- the UTC deadline is reached;
- any reviewed package or parent dependency changes version, source or multiplicity;
- one of the three advisory records disappears or changes ID, package,
  informational classification, security metadata or available remediation;
- either Cargo Deny configuration broadens or drops the reviewed exception list.

Every other advisory, license, source and banned-package rule continues to apply.
The separate cargo-audit job also remains enabled. Guard failures cannot be
overridden by a successful SARIF upload or report generation.

Run the same guarded check locally from the repository root:

```sh
cargo fetch --locked --manifest-path src-tauri/Cargo.toml
cargo deny --manifest-path src-tauri/Cargo.toml --config src-tauri/cargo-deny.toml fetch all
python3 scripts/check_rustsec_deferrals.py
cargo deny --offline --locked --manifest-path src-tauri/Cargo.toml --config src-tauri/cargo-deny.toml check advisories bans licenses sources
```

## Removing the deferral

Upgrade the notification, opener and Secret Service dependency paths together
once compatible maintained releases are available. Keep any Tauri JavaScript
and Rust plugin versions compatible. Verify existing Linux Secret Service keys,
notification delivery, file reveal, the Apple trust regressions and the native
desktop/mobile CI matrix. Then remove the three ignore entries and the temporary
guard after a fresh Cargo Deny scan succeeds without them. Do not silently extend
the deadline when an upstream release is still pending.
