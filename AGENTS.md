# Drip development guidelines

- code should be modular, concise, funtional and self-explaintory. minimize special cases, only do checks at user input and treat every internal code input/output correct.
- docs/comments are complementry to code. if a piece of code is not self-explaintory, especially for some dense kernel code, comment. also explain motivation when needed. do not write obvious/trivial things.
- cite when needed in IEEE format, such as algorithm borrowed from another paper or software.
- do not add any agentic coding system or model into co-authored-by.
- commits are self-contained, do one thing right, and has the format of "{submodule} or chore: {summary} \n {explanation (follow same guidelines as docs/comments)}"
- when some ideas are incomplete, conflicting or inheritly difficult to implement or require hackish methods, stop and talk to the user
- maintain a TODO.md, in which a table tracks ideas not implemented
- record decisions and rationale in `docs/DESIGN.md` with status: requirement, decided, tentative, or open. update `docs/PROGRESS.md` each session and track unimplemented ideas in the `TODO.md` table.
- style match existing code and/or commit
- maintain one source implementation of each computational kernel across execution backends; cross-platform support and ease of development take priority over peak performance.

## Project structure

- `crates/drip`: library. processing, node graph, color management, persistence. must never depend on a GUI.
- `crates/drip-cli`: batch frontend (binary `drip`), scaffold only.
- `crates/drip-gui`: interactive frontend (binary `drip-gui`) on winit + wgpu + egui: `display` (scRGB output), `preview` (wide-gamut image drawing), `editor` (node graph), `gui` (per-kind node bodies and pop-out windows), `widgets` (shared by node GUIs and the editor), `parent` (pop-outs above the main window), `inspector` (parameters, inputs), `theme`.
- `crates/drip-libraw`: minimal LibRaw binding (C shim + safe `decode`).
- `docs/DESIGN.md`: decisions, status, rationale. `docs/PROGRESS.md`: session log.
- `TODO.md`: ideas not implemented.
- `fixtures/`: test data; raws are stored with Git LFS.
