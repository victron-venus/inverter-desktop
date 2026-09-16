# Local response-lifetime fix

Source: `mac-usernotifications` 0.3.1 from crates.io, originally authored by
Hendrik Sollich. The original manifest, README, source, examples, and upstream
MIT / Apache-2.0 license notices are preserved. License files were retrieved
from https://github.com/hoodie/mac-usernotifications because the published
crate archive omits them.

Upstream `PendingGuard::into_receiver` uses `ManuallyDrop`, leaking its owned
request identifier and preventing pending-sender cleanup when a response
times out or is cancelled. This local patch retains the guard for the entire
response future and borrows its receiver. Normal Rust drop then cleans up all
completion paths, without changing the public API or notification delivery.
The sender is registered before enqueuing native work, and the worker checks
that its receiver remains alive before delivery. A cancelled submission cannot
register an orphaned sender later when the native worker resumes.

Four tests cover timeout completion, cancellation after polling, dropping an
unobserved notification handle, and cancellation before delayed native work.
They do not send native notifications.

Validation from the repository root on macOS:

```sh
cargo test --manifest-path src-tauri/vendor/mac-usernotifications/Cargo.toml --lib --no-default-features
```

Remove this vendored dependency after a reviewed upstream release contains
equivalent ownership and cancellation fixes.
