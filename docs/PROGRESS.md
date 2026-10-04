# Progress log

Newest first. Each entry: what happened, what is verified, what is next.

## 2026-10-04: M1 (graph engine and persistence)

- The user approved the design. Their decisions: JSON; templates as functions (F5); no WB state in types (P6); "no CAT step" reading (C3); export as u16/f32 with no compression or deflate (C5); toolkit winit + wgpu + egui (D4). Commit format: `{submodule}: …` or `chore: …`. The three scaffold commits were reworded to match.
- Display spike: scRGB through the Vulkan WSI is tagged correctly on KWin. Self-tagging as Rec.2020 also works but relies on undocumented WSI behavior, so we use scRGB and the compositor does the display transform (D2).
- Implemented M1 in `crates/drip`: `value` (semantic port types), `param` (schemas, JSON values), `node` (kinds, registry, actions, migration), `graph` (validated editing), `eval` (pull evaluation, dependency stamps, per-node errors, full-resolution actions that release intermediates), `project` (graph inputs and arguments, JSON format, migration, opaque nodes).
- Codex reviewed M1 and found 11 issues. Fixed 10. Resource revisions (E2) are deferred to M2.
- Verified: 30 integration tests with toy node kinds. A mutation check confirmed the memory-release test catches regressions. clippy and fmt are clean.
- Next: M2. LibRaw C shim, raw.read with resource revisions, the prototype nodes, LittleCMS TIFF export.

## 2026-10-04: session 1

- Set up the workspace (`drip`, `drip-cli`, `drip-gui`), CI (Linux fmt/clippy/test; Windows and macOS build), AGENTS.md, DESIGN.md and TODO.md. License AGPL-3.0-or-later. git initialized, nothing committed yet.
- Surveyed LibRaw bindings: none is both sound and maintained (DESIGN L2). Proposed our own C-shim binding.
- Investigated the display path on the dev machine (DESIGN section 6): KWin is parametric-only and color-manages untagged surfaces as sRGB. NVIDIA's Vulkan WSI offers BT2020_LINEAR and scRGB float swapchains. GTK 4.22 didn't tag a Rec.2020 surface in a spike.
- Wrote the proposed core design (types, kinds and instances, pull evaluation with stamps, views and actions, persistence) for the user to review. Implementation waits until it is agreed.
- Codex (gpt-6-astra, high) reviewed the design. Its corrections are folded into DESIGN.md: matrix direction, black handling, limits of LittleCMS unbounded mode, scRGB and egui alpha details, resource revisions, port migration. Its open points are listed as open entries (P6, C3, C5).
- Next: user review of DESIGN.md, agree on milestones, then M1.
