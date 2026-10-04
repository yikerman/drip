# Drip Dev Guidelines

- code should be modular concise, funtional and self-explaintory. minimize special cases, only do checks at user input and treat every internal code input/output correct.
- docs/comments are complementry to code. if a piece of code is not self-explaintory, especially for some dense kernel code, comment. also explain motivation when needed. do not write obvious/trivial things.
- cite when needed in IEEE format, such as algorithm borrowed from another paper or software.
- do not add any agentic coding system or model into co-authored-by.
- commits are self-contained, do one thing right, and has the format of "{submodule} or chore: {summary} \n {explanation (follow same guidelines as docs/comments)}"
- when some ideas are incomplete, conflicting or inheritly difficult to implement or require hackish methods, stop and talk to the user
- maintain a TODO.md, in which a table tracks ideas not implemented

## Working agreements

- be a collaborator, not a silent implementer. when an idea is incomplete, conflicts with another or with how a platform/library behaves, is inherently difficult, or the only way forward is hackish (patching/forking a dependency, undocumented behavior, fighting a library's design, special cases breaking uniformity): explain plainly, give options with trade-offs and a recommendation, let the user decide, and continue with work that doesn't depend on the answer.
- the user wants designs proposed and agreed before substantial implementation.
- keep the prototype minimal: CPU only, no premature optimization, no speculative abstractions. invest in getting the core abstractions right instead of extra features.
- C/C++ dependencies are fine if their Rust bindings are sound; check that the safe API upholds memory and thread safety and report doubts.
- before substantial work, say briefly what you will do; afterwards recap what changed, what was verified, what is open.
- record every decision in `docs/DESIGN.md` with status (requirement, decided, tentative, open) and rationale; append to `docs/PROGRESS.md` at the end of each session.
- ask before destructive or hard-to-reverse actions (deleting files, rewriting git history, pushing).
- tests check real behavior. if a test or requirement seems wrong, say so instead of working around it.
- raw decoding: LibRaw only. color management: LittleCMS via `lcms2` only. raise real problems with them, do not propose alternatives.

## Second opinion with Codex

Significant design decisions and substantial implementations get a review from the Codex CLI before they're called done (skip routine changes):

    codex exec -m gpt-6-astra -c model_reasoning_effort=high "<prompt>"

Treat it as a second opinion, not an authority: adopt what is right, briefly explain what is set aside, bring real disagreements to the user, and tell the user what Codex raised and what was done about it.

## Project structure

- `crates/drip`: library. processing, node graph, color management, persistence. must never depend on a GUI.
- `crates/drip-cli`: batch frontend (binary `drip`), scaffold only.
- `crates/drip-gui`: interactive frontend (binary `drip-gui`), placeholder until the toolkit is chosen.
- `docs/DESIGN.md`: decisions, status, rationale. `docs/PROGRESS.md`: session log.
- `TODO.md`: ideas not implemented.
