# assessment-core (vendored)

The change-impact engine the backend uses to version issues and projects and to assess every
change to them (`src/engine/impact.rs`). Upstream:
[assessment-engine](https://github.com/f8fvwgzc/assessment-engine), crate
`crates/assessment-core`, version 0.1.0. The scoring is documented upstream in
`docs/SCORING.md`.

It is vendored, not a git or registry dependency, because the backend image builds from
`./backend` alone (`backend/Dockerfile`) and the engine is not published yet. Once it is, replace
this directory with `assessment-core = { git = "…", rev = "…" }` (and allow the source in
`deny.toml`).

To update it, copy `crates/assessment-core/src` over `src/` here, carry over any dependency
change from the upstream manifest into `Cargo.toml`, and run the backend's tests. Do not edit
the sources here: change them upstream and copy them again.
