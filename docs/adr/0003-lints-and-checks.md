# Lints and checks

The workspace lints in `Cargo.toml` are the Rust counterpart of basedpyright's strict mode. `rustc` already checks types, and the lints add the checks basedpyright layers on top. The explicit denies mirror Zed's `[workspace.lints]`. Warnings are errors in CI (see ADR 0002 and `scripts/clippy`).

## Suppressions

`allow_attributes` and `allow_attributes_without_reason` make every suppression use `#[expect]` with a reason, like basedpyright's `reportUnnecessaryTypeIgnoreComment`. An `#[expect]` fails once its lint no longer fires, so stale suppressions get removed.

## Pedantic lints turned off for the whole workspace

These pedantic lints fight GPUI code more than they help:

- `unreadable_literal`: colors are written as `0xRRGGBB` literals.
- `cast_precision_loss`: layout math turns `usize` counts into `f32`.
- `type_complexity`: GPUI callback types are long (as in Zed).
- `wildcard_imports`: child modules use `use super::*`, and views use the GPUI prelude.
- `enum_glob_use`: large action matches use `use Action::*`.
- `default_trait_access`: `..Default::default()` is the GPUI and Zed idiom.
- `redundant_closure_for_method_calls`: builder closures such as `.when(c, |d| d.pl_6())` read better than method paths.
- `unused_self`: render helpers stay methods on their view.
- `missing_errors_doc` and `missing_panics_doc`: the crates are internal, not a published API, so their public functions skip the `# Errors` and `# Panics` sections a library documents.

## Spelling

`.config/typos.toml` leaves out text that has to keep its original spelling: the ACP registry copy (`assets/registry.json`), third-party agent skills, and research notes, which quote their sources. `porfolio` is the upstream name of a theme.

## Unfinished work

`scripts/check-todos` fails when committed code holds unfinished-work markers (from Zed). Its pattern writes one letter in brackets, so the script itself doesn't show up when someone searches for the markers.
