# Drip development guidelines

- code should be modular, concise, funtional and self-explaintory. minimize special cases, only do checks at user input and treat every internal code input/output correct.
- docs/comments are complementry to code. if a piece of code is not self-explaintory, especially for some dense kernel code, comment. also explain motivation when needed. do not write obvious/trivial things.
- Write code docs for engineers familiar with Rust, image processing and colorimetry. Focus on project-specific contracts and rationale; omit generic tutorials, repetition and document housekeeping.
- cite borrowed algorithms and papers in IEEE format. Maintain `THIRD_PARTY.md` with credits and licenses for major dependencies and adapted code, pinned revisions for source ports, and full references for papers. Keep upstream notices and brief citations beside adapted algorithms; explain meaningful deviations there. Link upstream license texts from `THIRD_PARTY.md` rather than duplicating them in a `LICENSES/` directory.
- do not add any agentic coding system or model into co-authored-by.
- commits are self-contained, do one thing right, and has the format of "{submodule} or chore: {summary} \n {explanation (follow same guidelines as docs/comments)}"
- when some ideas are incomplete, conflicting or inheritly difficult to implement or require hackish methods, stop and talk to the user
- maintain a TODO.md, in which a table tracks ideas not implemented
- record decisions and rationale in `docs/DESIGN.md` with status: requirement, decided, tentative, or open. update `docs/PROGRESS.md` each session and track unimplemented ideas in the `TODO.md` table.
- style match existing code and/or commit
- maintain one source implementation of each computational kernel across execution backends; cross-platform support and ease of development take priority over peak performance.

## User-facing documentation

- Keep node help in the frontend, separate from code comments. Show it below the type ID in the main inspector, not in pop-outs.
- Briefly state the operation, assumptions and inputs/outputs. Be technical and precise: prefer `x * 2^ev` to vague brightness advice. Link references for complex algorithms instead of expanding their derivation.
- Use compact shared names such as `scn rec2020 img` and `disp rec2020 img`, both implicitly linear. Spell out exceptional encodings or requirements where relevant.
- Follow darktable's module-reference structure without its length. Use short, direct sentences. Avoid filler, repeated explanations and unnecessary semicolons.

## Project structure

- `crates/drip`: library. processing, node graph, color management, persistence. must never depend on a GUI.
- `crates/drip-cli`: batch frontend (binary `drip`), scaffold only.
- `crates/drip-gui`: interactive frontend (binary `drip-gui`) on winit + wgpu + egui: `display` (scRGB output), `preview` (wide-gamut image drawing), `editor` (node graph), `node_ui` (per-kind node bodies and pop-out windows), `editing` (graph edits and notifications), `widgets` (shared by node GUIs and the editor), `parent` (pop-outs above the main window), `inspector` (parameters, inputs), `theme`.
- `crates/drip-libraw`: minimal LibRaw binding (C shim + safe `decode`).
- `docs/DESIGN.md`: decisions, status, rationale. `docs/PROGRESS.md`: session log.
- `TODO.md`: ideas not implemented.
- `THIRD_PARTY.md`: dependency and algorithm credits, licenses, and paper references.
- `fixtures/`: test data; raws are stored with Git LFS.
