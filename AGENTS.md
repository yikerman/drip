# Drip development guidelines

## Philosophy

- a DAG refuses to evaluate iff something is logically inherently wrong,
  where logical validity is more permissive than intuition, e.g. in image
  processing when creative intent is involved. pipeline recommendations are
  guidance.

- enforce port contracts when connecting and data-dependent contracts when
  evaluating.

## Code organization

- code should be single-source-of-truth, modular, concise, functional and
  self-explanatory. check inputs at boundaries, trust internal code afterwards.
  keep abstractions proportional to actual needs.

- a node computes context × parameters × inputs → outputs. 
  where computation is pure. define contracts and docs with the node,
  generate mechanical discovery and bindings.

- compose reusable operations over values. keep loading, processing and
  presentation separate.

- in Rust, structs represent memory layouts and/or interpretations; traits
  express interpretation contracts. interpretations determine compatibility.

- maintain one source implementation of each computational kernel across
  execution backends. cross-platform support and ease of development take
  priority over peak performance.

- match existing code style and commit message.

- discuss incomplete or conflicting requirements, and difficulties requiring
  design tradeoffs or hackish workarounds.

## Documentation

- docs/comments complement code. explain contracts, motivation and dense kernels.
  write for engineers familiar with Rust, image processing and colorimetry.
  keep prose short, direct and specific to the project.

- define node help in Rustdoc on the node function, with references in
  `#[node(...)]`. keep implementation rationale in ordinary comments. show help
  below the type ID in the main inspector.

- briefly state operation, assumptions and inputs/outputs. use declared payload
  names on ports and document creative reinterpretations. follow darktable's
  module-reference structure, kept concise. link algorithm derivations. write
  like a human, no common ai slop or filler.

- cite borrowed algorithms and papers in IEEE format. maintain `THIRD_PARTY.md`
  with dependency/adapted-code credits and licenses, pinned source-port revisions
  and full paper references. keep upstream notices and brief citations beside
  adapted code; explain meaningful deviations there. link upstream license texts.

## Workflow

- record decisions and rationale in `docs/DESIGN.md` with status: requirement,
  decided, tentative, or open. update `docs/PROGRESS.md` each session. track
  unimplemented ideas in the `TODO.md` table.

- commits are self-contained and do one thing right. format:
  `{submodule} or chore: {summary}` followed by an explanation using the same
  guidelines as docs/comments. do not add coding agent or model to
  co-authored-by.

## Project structure

- `crates/drip`: headless library. processing, node graph, color management,
  persistence. think of what is reachable on a headless batch processing system:
  if code has parts a headless system will never touch (e.g. prepare data for
  gui vectorscope) it should not be there.

- `crates/drip-cli`: batch frontend todo

- `crates/drip-gui`: interactive frontend (binary `drip-gui`) on winit + wgpu +
  egui. owns GUI code, presentation and all preview preparation.

- `crates/drip-libraw`: minimal LibRaw binding (C shim + safe `decode`).

- `fixtures/`: test data; binary data are stored with Git LFS.
