# Progress log

Newest first. Each entry: what happened, what is verified, what is next.

## 2026-10-04: session 1

- Set up the workspace (`drip`, `drip-cli`, `drip-gui`), CI (Linux fmt/clippy/test; Windows and macOS build), AGENTS.md, DESIGN.md and TODO.md. License AGPL-3.0-or-later. git initialized, nothing committed yet.
- Surveyed LibRaw bindings: none is both sound and maintained (DESIGN L2). Proposed our own C-shim binding.
- Investigated the display path on the dev machine (DESIGN section 6): KWin is parametric-only and color-manages untagged surfaces as sRGB. NVIDIA's Vulkan WSI offers BT2020_LINEAR and scRGB float swapchains. GTK 4.22 didn't tag a Rec.2020 surface in a spike.
- Wrote the proposed core design (types, kinds and instances, pull evaluation with stamps, views and actions, persistence) for the user to review. Implementation waits until it is agreed.
- Codex (gpt-6-astra, high) reviewed the design. Its corrections are folded into DESIGN.md: matrix direction, black handling, limits of LittleCMS unbounded mode, scRGB and egui alpha details, resource revisions, port migration. Its open points are listed as open entries (P6, C3, C5).
- Next: user review of DESIGN.md, agree on milestones, then M1.
