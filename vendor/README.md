# Vendored dependencies

Three crates are copied here verbatim from
[Rune](https://github.com/anomalyco/rune) at the revision pinned in the root
`Cargo.toml` (`631e8a6`, `dev`):

| Directory | Upstream crate | Why |
| --- | --- | --- |
| `parley_engine/` | `parley_engine` 0.12 | Rune needs the bidi rule L1 fix and the cached shaping, and asks for a `line-dictionaries` feature that the published crate does not have. |
| `wgpu-hal/` | `wgpu-hal` 30.0.1 | Rune calls `dx12::Instance::expose_adapter` and `Surface::set_present_rects` (partial presents). |
| `winit/` | `winit` 0.30 | Rune calls the Windows taskbar and window-placement fixes. |

A dependency's `[patch]` section does not apply to a workspace that depends on
it, so an app that builds Rune has to patch the same crates itself. These are
the patched sources, not the published ones, and they are the ones Rune's own
`Cargo.toml` points at.

Rules:

- Do not edit these files. `cargo fmt` and `cargo clippy` skip them because they
  are not workspace members.
- Bump them in the same commit as the Rune revision, by re-copying from
  `vendor/` in the new revision. Do not mix revisions.
- Licenses are unchanged from upstream: `winit` and `wgpu-hal` are MIT/Apache-2.0,
  `parley_engine` is MIT/Apache-2.0.
