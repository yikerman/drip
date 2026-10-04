# Drip: handoff to Claude Code

You are joining a project as its engineer, working directly with its author & designer (the user) in Claude Code. The project is **Drip**, short for **Done Right Image Processing**: a node-based computational photography application written in Rust.

This document summarizes a design conversation the user had with Claude in the chat app. It is a starting point, not a specification. The requirements below are the user's goals; everything else is unverified thinking that you're expected to examine, challenge, and refine together with the user. Library details mentioned here may be outdated, so check current documentation before relying on them.

<working_agreements>
The user wants a collaborator, not a silent implementer. Stop and talk to the user when an idea is incomplete, when ideas conflict with each other or with how a platform or library actually behaves, when something is inherently difficult, or when the only way forward you can see is hackish (patching or forking a dependency, relying on undocumented behavior, fighting a library's design, or special cases that break the architecture's uniformity). Explain the problem plainly, lay out the options with their trade-offs, give your recommendation, and let the user decide, while continuing with work that doesn't depend on the answer. The user would much rather hear about a problem early than find out later that it was quietly worked around.

Also:
- Keep the prototype minimal: CPU-only processing, no premature optimization, no speculative abstractions. The user values expressive, well-designed abstractions, so put effort into getting the core ones right rather than into extra features.
- C and C++ dependencies are fine as long as their Rust bindings are sound. When choosing a binding, check that its safe API actually upholds memory and thread safety, and tell the user about any doubts.
- Before a substantial piece of work, say briefly what you're about to do; afterwards, recap what changed, what you verified, and what's still open.
- Keep a design notes document in the repository that records each decision, its status (requirement, decided, tentative, open), and its rationale, plus a short progress log for resuming in fresh sessions. Put these working agreements and the Codex practice below into the project's CLAUDE.md.
- Ask before destructive or hard-to-reverse actions such as deleting files, rewriting git history, or pushing.
- Write tests that check real behavior. If a test or requirement seems wrong, say so rather than working around it.
</working_agreements>

<second_opinion_with_codex>
Have OpenAI's Codex CLI, using the model **gpt-6-astra** at **high** reasoning effort, review your significant design decisions and any substantial implementation before you call it done. Skip it for routine changes.

Treat its review as a second opinion, not an authority. Adopt what's right, briefly explain what you're setting aside, bring real disagreements to the user, and tell the user what Codex raised and what you did about it.
</second_opinion_with_codex>

<requirements>
**Structure.** Three parts:
- a **library** containing all processing, the node graph, color management, and persistence, with no dependency on any GUI;
- a **CLI** for batch processing, for example applying a project or template to many raw files. Scaffold it now so the library's API is shaped with it in mind, but postpone implementing it;
- a **GUI** for building and tuning node graphs interactively.

The GUI and CLI should be thin frontends over the library, and both must work with the same project and template files.

**Platforms.** Linux on Wayland is the primary platform: develop, test, and verify color accuracy there first. Windows and macOS remain targets, so keep the code portable and the builds working, but test them only loosely and defer their platform-specific work. X11 is not supported.

**Libraries.** Raw decoding uses LibRaw and nothing else. Color management uses LittleCMS through the lcms2 crate and nothing else. Don't propose alternatives, but do raise real problems you run into with them.

**Processing model.** Computation is a directed acyclic graph of nodes. Each node has an arbitrary set of typed inputs and outputs, conceptually a typed tuple each. Evaluation topologically sorts the graph and recomputes a node only when it or something upstream has changed. Connections are type-aware, so a user can't connect incompatible ports, such as raw metadata into an input expecting a linear Rec.2020 float image.

**UI-only nodes.** Some nodes have no outputs and only present something in the UI, such as an image preview or a histogram. They usually sit near the end of a pipeline, but users may place them anywhere, and nothing in the design should special-case or prevent that.

**Precision and preview.** Processing buffers are 32-bit float. Interactive editing works from a scaled-down version of the raw image.

**Color.** The GUI composites everything in linear Rec.2020. The only output format is TIFF, with an arbitrary user-chosen ICC output profile embedded in the file. The display path supports arbitrary display profiles: where the compositor can perform the final transformation to the display, use it (on some platforms it's the only option); where it can't, the application performs it.

**Persistence.** The entire graph is serializable to a project file. Users can start projects from templates and save their own graphs as templates.

**First prototype.** A minimal raw-to-TIFF pipeline: a raw reader that takes a file path and outputs the Bayer mosaic (scaled by black and white points) and metadata; white balance from the camera's as-shot multipliers, with no chromatic adaptation; debayering by naive 2×2 binning; conversion from camera RGB to linear Rec.2020; a simple sigmoid tone mapper; TIFF export; and a Rec.2020 preview and a histogram as UI-only nodes.
</requirements>

<ideas_from_the_chat>
Unverified. Many came from the assistant rather than the user.

- **GUI toolkit:** open, but prefer the simplest and maintainable choice. The app should probably own its window and GPU surface (winit and wgpu) so it controls the surface's color space, with egui or iced drawn on top; with Linux as the priority, GTK4 is also worth considering. Known difficulty: these toolkits' renderers assume an sRGB target, so drawing UI into a linear wide-gamut surface may require changes to the renderer. For the first prototype, the preview could simply show 8-bit sRGB and defer the proper display path.
- **Export:** output profiles are display-referred, so tone mapping must come before the output transform.
- **Port types:** semantic rather than structural. A camera-RGB image and a Rec.2020 image share a memory layout but should be different port types; the Bayer mosaic should carry its CFA pattern. Some inputs, like a histogram's, may accept several types. (TristimulusMatrix trait?)
- **UI-only nodes and the CLI:** these nodes belong to the graph and project files, but their presentation belongs to the GUI. The CLI must load and run the same graphs without them getting in the way. Side effects such as writing files should happen on explicit action, not on every re-evaluation.
- **Persistence:** stable node IDs, ports referenced by name, format versioning, and tolerance for missing parameters and unknown node types, so files survive the app's evolution. Templates strip project-specific parameters such as input paths.
</ideas_from_the_chat>

<later_not_now>
Don't build these yet, but don't design them out: implementing the CLI, GPU processing, background evaluation, full-resolution processing of the visible region for 1:1 viewing, EXIF passthrough, soft-proofing, and Windows and macOS display color paths.
</later_not_now>

<open_questions>
- Which GUI toolkit.
- How to bind and link LibRaw, any well-maintained binding?
- Project file format.
</open_questions>

<first_session>
1. Set up the repository: the library, CLI, and GUI as separate parts of one workspace, the design notes built from this handoff, a progress log, AGENTS.md (DO NOT USE CLAUDE.md), and CI that tests on Linux and builds on Windows and macOS.
2. Tell the user which parts of the design you consider weakest, contradictory, or riskiest, and which open questions block the first milestone. Get a Codex review of that assessment first.
3. Agree on a milestone plan with the user. One reasonable order: the graph engine and persistence in the library, tested headlessly; the prototype nodes producing a TIFF from a test harness; the GUI; then the proper display color path on Wayland.

Don't start the GUI until the toolkit has been chosen with the user.
</first_session>
