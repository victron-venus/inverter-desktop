# Local glib 0.18.5 patch

This is the crates.io `glib` 0.18.5 source, preserving its MIT license. Only
`src/variant_iter.rs` differs in production source: the C out-pointer is declared
mutable and passed as `&mut p`. This is the exact change from gtk-rs
[PR 1343](https://github.com/gtk-rs/gtk-rs-core/pull/1343), merge
`05dff0ee696f9bcd8617cd48c4b812d046d440cb`, addressing RUSTSEC-2024-0429.
`INVERTER-PATCH.json` records registry provenance and changed-file hashes.

The published 0.18 series ends at 0.18.5; the fixed 0.20 API cannot replace it in
Tauri's GTK dependency graph by a patch-version update. The path patch preserves
that graph. Remove it when all consumers use a fixed compatible upstream crate.
The version is deliberately unchanged: version-based scanners may still report
the informational unsoundness advisory; inspect this backport instead of adding
a blanket ignore. Unmaintained upstream dependencies are a separate concern.

`tests/glib-variant-regression` (at repository root) invokes the real patched
crate in optimized release mode. Tests cover forward/backward iteration,
`nth`/`nth_back`, `last`, empty/exhausted iterators and Unicode. CI runs this on
Linux after installing GLib/GTK prerequisites. The original immutable pointer
has undefined behavior when the native variadic function writes through it;
source search finding no current application caller is not a safety argument.
