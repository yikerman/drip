# Type-system redesign: user statements

Recorded 2026-10-08. This file separates the user's requirements and examples
from the analysis in [the design study](TYPE_SYSTEM_REDESIGN_2026-10-08.md).
Summaries below are paraphrases unless quoted. They do not imply acceptance of
an assistant proposal.

## Working instructions

| ID | User statement | Status |
| --- | --- | --- |
| U01 | The current type system is incorrect; a complete redesign is needed. The earlier analysis failed to identify the problem. | User assessment; this study is not an audit of the old implementation. |
| U02 | Do not treat existing project documentation or code as authoritative. | Requirement. |
| U03 | Document the new designs, reasoning, and proposals in a separate file under `agent-docs`; do not write to existing logs. | Requirement; overrides the usual updates to `DESIGN.md`, `PROGRESS.md`, and `TODO.md` for this work. |
| U04 | Keep track of the user's statements separately. | User suggestion, adopted by creating this file. |
| U05 | “also always challenge my ideas” | Requirement for the discussion. |
| U06 | The initial task concerns concepts, not a Rust implementation. | Initial scope; R09 later requests a Rust type-system formulation, not production integration. |

## Fixed design constraints stated by the user

| ID | Constraint | Qualification in the prompt |
| --- | --- | --- |
| F01 | Accelerate DAG computation using WGSL or CubeCL. | Select after understanding the abstraction. |
| F02 | Assume most processing nodes execute on the GPU. Keep intermediate data resident in device memory. | Host/device synchronization and transfers are expensive. |
| F03 | Do not depend on kernel fusion. | Building the required compiler is beyond scope. |
| F04 | Every node remains a pure function: `(global context, parameters, input ports) -> output ports`. | Nodes and parameters carry documentation and other metadata. |
| F05 | Each port carries a concrete buffer or data structure with CPU/GPU representations. | The representation need not have the same memory layout on both. |
| F06 | For each supported logical type, there should be a bijection between its CPU and GPU representations. | Detailed implementation remains undecided. |
| F07 | Kernel inputs and outputs must have concrete Rust data types known at compile time. | A node should not receive a very abstract algebraic structure instead of a concrete representation. |
| F08 | Port values still require interpretation/assumption/capability checks. | Their definitions are the main subject of this investigation. |
| F09 | The graph comprises nodes and edges, is acyclic, and evaluates in dependency order. | Concurrent dispatch of independent kernels is an open possibility. |
| F10 | The DAG knows node metadata and initially does not keep an evaluation cache. | Large image buffers risk exhausting host/device memory. |
| F11 | Roughly 4 GB of memory is a reasonable working scale. | This is a scale assumption, not a stated per-image limit or a detailed budget policy. |
| F12 | The framework should remain compatible with future caching. A raw-reader cache is useful because disk I/O and decoding are expensive. | The user suggests internal caching can preserve purity while changing performance and memory use. |
| F13 | Connecting ports requires checks. | Which checks is still open. |
| F14 | Evaluation is demand-driven: `eval(dag, global context, target node)`. | Requests may originate from the GUI, export, or other consumers. |

These entries preserve the user's designation “fixed designs.” Objections to
their literal formulations belong in the study, rather than silently revising
this record.

## Illustrative cases and desired behavior

| ID | User example | Qualification |
| --- | --- | --- |
| C01 | Demosaiced camera RGB and Rec.2020 images can have the same three-channel tensor layout, while color grading or sigmoid on camera RGB was described as making no sense. | Illustrates the need for interpretations beyond layout; the categorical claim is to be challenged. |
| C02 | A color-calibration module before camera-to-Rec.2020/XYZ conversion or after sigmoid was described as making no sense. | Intended as a challenge to the old treatment of “linearity,” not a fully specified calibration algorithm. |
| C03 | The prompt describes scene-referred/display-referred as proportional to scene light/intended display light. | Terminology and proportionality claims are to be examined independently. |
| C04 | A mosaic is a two-dimensional scalar array, with values informally related to collected photons and sensor capacity. | The user explicitly notes QE, shot noise, read noise, and other information losses. |
| C05 | “fraction of full-well capacity plus mosaic pattern” is tempting as an interpretation, but may claim excessive physical accuracy. | The correct minimum interpretation is open. |
| C06 | Missing dark/flat fields, sensor profiles, and other calibration data should not prevent ordinary processing. Later nodes may operate on a sufficient approximation. | This includes as-shot white balance and highlight recovery. |
| C07 | Fully accurate physical information cannot generally be obtained. | Approximation is intrinsic, not an exceptional failure case. |
| C08 | Reject DAGs if and only if they are logically wrong, while allowing creative processing. | Initial target; the follow-up R01 relaxes coverage. |

The user explicitly says the examples are illustrative, informal, and not
fully accurate descriptions. The sentence beginning “suppose a color grading
tool receives somethings, transforms” is unfinished; no additional requirement
is inferred from it.

## Requested first deliverable

1. Challenge the prompt and identify unsound assumptions.
2. Consult common darktable and vkdt modules.
3. Derive their minimum interpretations, assumptions, and capabilities from
   physics and mathematics, using primary references where useful.
4. Propose what to define and how modules request concrete port types and
   semantic requirements, consistent with rejecting logical errors.

## Follow-up: practical coverage

Recorded 2026-10-08, after the first study.

| ID | User statement | Status |
| --- | --- | --- |
| R01 | Aim for practicality and “strictly only forbid a subset of \"logically wrong\"” cases. | User-proposed revised target; complete detection is no longer the objective. |
| R02 | Catch most ordinary errors while leaving algorithm nuances and similar details out. | Desired coverage; the concrete error set and exclusions remain to be specified. |
| R03 | vkdt's buffer checks are too loose; Drip should check substantially more while remaining practical and clean. | Desired direction. The characterization of vkdt is a claim to investigate, not an assumed source fact. |
| R04 | Inspect concrete nodes in darktable and vkdt to establish examples. | Requested basis for the practical proposal. |
| R05 | Do not assume darktable or vkdt solved these problems correctly. Identify their tradeoffs that Drip need not make under a checked framework. | Requirement for how to use upstream examples; their current restrictions and workarounds are not automatically Drip requirements. |
| R06 | “we should minimize branching, and each algorithm do one thing right” | Requirement direction. The analysis must distinguish algorithm choices from parameters and necessary internal control flow. |
| R07 | Bayer demosaic and X-Trans demosaic should be two separate nodes; they are different algorithms. | Explicit node separation, not merely separate implementations behind one node. |
| R08 | “there should be no guessing in a node” | Explicit clarification. A node executes its selected operation against its declared contract; incompatible input does not cause automatic algorithm substitution. |
| R09 | The separate-node direction looks plausible; formulate how to embed it in Rust's type system, starting from concrete CPU/GPU buffers. | Requested next design step; not blanket approval of all details in the earlier proposal. |
| R10 | DAG checks run on the CPU. Metadata, interpretations, and constraints can be logically grouped with a buffer while physically separate from image storage. | Requirement and proposed representation direction. |
| R11 | Connections that do not pass checks should be unconnectable at “connect time,” distinct from Rust compile time and evaluation time. | Desired enforcement point; feasibility must be examined. |
| R12 | If strict connect-time checking is not possible, consider relaxing it by moving some checks to evaluation time. | Authorized fallback to investigate, not approval to defer all checks. |
| R13 | Evaluation is responsible for CPU/GPU data movement because the required placement is known at connect time. | Explicit responsibility and timing constraint. |
| R14 | “node should handle the minimum work possilbe” | Keep transport and placement handling out of the computational node; exact API remains to be formulated. |
| R15 | Give an exhaustive payload list/examples based on the darktable and vkdt survey. | Requested catalogue; R17 clarifies that the system must remain extensible, rather than using a final closed payload universe. |
| R16 | Do not pack everything into one buffer or into metadata. A node can produce several outputs, including a color-transform matrix or LUT. | Explicit correction: independently usable computational values belong on separate typed ports. |
| R17 | Some outputs may be optionally reused; think about an extensible system. | Requirement for independent connections/reuse and an extensible payload design. Exact unused-output execution policy remains open. |
| R18 | Recognize that vkdt may be doing something wrong. | Reiterates R05: inspect and challenge its architectural choices; do not inherit them merely because they exist. |
| R19 | Ask whether interpretation remains in the design or has been replaced/discarded. | Requested clarification, not an instruction to remove interpretation. |
| R20 | “list some examples of node definiton” | Requested concrete examples under the current Rust, interpretation, multi-port, and evaluator-owned transport design. |
| R21 | Prefer `#[node(..)]`, like the existing drip macro. | Proposed declaration style; existing implementation semantics remain subject to redesign. |
| R22 | `describe` should instead be named `contract`, `subject to`, or similar. | Requested contract-oriented terminology, not a final choice between those names. |
| R23 | Give an example of such a check. | Requested concrete checking logic rather than only a declaration or an opaque checking helper. |
| R24 | Work out DAG and evaluation under the discussed design, recalling previous constraints. | Requested next topic; R25 pauses this work before a concrete implementation design. |
| R25 | Stop DAG/eval design because specific implementation needs CPU/GPU runtime knowledge; design a benchmark first. | Current priority. Cover real algorithms ported from existing code and synthetic residency, host endpoints, CPU islands and related workloads. |
| R26 | Show the benchmark set, then launch two subagents to explore WGSL versus CubeCL, model fit, simplicity and maintainability, with reasonably optimized kernels and profiling. | Explicit authorization for two backend agents after presenting the shared benchmark. Requested metrics include memory/computation usage, GPU utilization, scheduling and synchronization overhead. |
| R27 | “when you are done, put results into plots with latex” | Requested final benchmark presentation: LaTeX plot sources and rendered figures. |
| R28 | “reasonable and correctness is enough, dont over-optimize” | Prioritize correctness and useful measurements. Limit optimization to straightforward workgroup/access/allocation choices; no exhaustive tuning requirement. |
| R29 | “slight errors are fine” | Small numerical deviations are acceptable. Applied to the observed sparse RCD CPU/GPU discrepancy: retain its measured magnitude and original strict-check result, but do not treat that result as a benchmark blocker or require identical floating-point results. No universal numerical error bound was specified. |
| R30 | “i assume cubecl can use cuda? try it” | Extend the existing CubeCL experiment to its CUDA runtime and measure it. |
| R31 | User installed CUDA 13.4 (`nvcc` V13.4.92) and Nsight Compute 2026.3.1, then said “ok”. | Resume the CUDA experiment with the now-available tools. The preceding wait was for installation, not cancellation. |
| R32 | “it should be a small change, since cubecl is designed this way” | Reuse the computational kernels; keep changes at runtime selection and adapters where possible. |
| R33 | “also make subagent high instead of xhigh” | Use high reasoning effort for the CUDA subagent. |

| R34 | “skip it, we know cuda is somewhat better is enough” | Stop further CUDA profiling; no administrative profiler changes. |
| R35 | Ask whether CubeCL is preferable and how it interacts with the frontend. | Evaluate framework choice separately from CUDA/graphics interoperability. |
| R36 | Read all design logs and start a CubeCL migration; initially requested a standalone `cubecl-experiment` directory. | Superseded by R41: rewrite the existing crates directly. |
| R37 | Move benchmark results elsewhere so they are not included in Git. | Requirement; artifacts and retired benchmark sources are archived outside the repository. |
| R38 | First build a working, correct, clean node/port/DAG/eval model and macro facility; only then adapt UI, only then consider optimization. | Required implementation order. |
| R39 | “do not optimize prematurely”; remember AGENTS.md and do not overcomplicate. | Requirement. |
| R40 | The PoCs have established feasibility; `experiments/` is no longer needed, and this is a large project rewrite. | Retire the PoC runners from the active tree; preserve source evidence externally. |
| R41 | “rewrite drip-* directly, integrating every, and start a git fork” | Corrects the assistant's mistaken separate-core layout. Use the existing project structure. |
| R42 | Clarified that the Git fork means a local branch named `cubecl-rewrite`. | Explicitly authorized; branch created from `master`, no hosted fork or push. |

This narrows C08. It does not approve any particular metadata scheme,
blacklist, or rule proposed by the assistant. The response to this direction
is recorded in [the practical proposal](TYPE_SYSTEM_REDESIGN_PRACTICAL.md).

## Discussion state

The user has proposed a narrower enforcement objective. The broad evidence
tracking in the first study is not an accepted implementation requirement.
The user requires separate Bayer and X-Trans demosaic nodes without guessing
inside either node. Earlier assistant suggestions of an `Auto` processing
node or mode-dependent demosaic contracts are withdrawn.
The next proposal is [the Rust formulation](TYPE_SYSTEM_REDESIGN_RUST.md).
It investigates connect-time enforcement and reserves CPU/GPU movement for
the evaluator, with placements declared before evaluation.
The [port-payload clarification](TYPE_SYSTEM_REDESIGN_PORT_PAYLOADS.md)
records multi-output nodes, independent calibration/transform values, optional
reuse, and payload extension. An image descriptor is not a container for a
node's auxiliary results.
The port-payload document now makes interpretation explicit within descriptions
and includes illustrative attribute-based node declarations and an explicit
matrix/image interpretation comparison. `contract = ...` and parameterized
matrix endpoint families are assistant proposals in response to R21–R23.
The declaration syntax and exact
contracts are proposals, not implemented APIs or user-approved definitions.
Backend selection, CPU/GPU equivalence, the selected checks, and the concrete
metadata vocabulary remain open.
The next work is the [runtime benchmark](TYPE_SYSTEM_REDESIGN_BENCHMARK.md).
DAG/evaluator implementation design is paused while the two runtime experiments
provide evidence. The benchmark workloads and methodology are assistant proposals;
the request does not pre-approve a runtime or a specific evaluator architecture.

- R43 (requirement): after implementation and checks, independent review by Astra at high reasoning, with minimal prior design rationale. Question everything; explicitly check whether abstraction infrastructure can be simplified, code smells, and correctness.

- R44 (requirement): delegate unreadable drip-macros refactoring; long, nested functions across the project should trigger a responsibility/simplification review, not cosmetic helper extraction.

- R45 (requirement/correction): preserve existing major GUI behavior, especially universal node controls/views and per-node pop-out windows. The frontend change is an internal adaptation to the new model, not a GUI redesign.

- R46 (requirement/clarification): internal implementation may change freely. Node documentation and contract rendering may change where the new architecture calls for it. Preserve the general GUI design and interaction logic; preservation does not require retaining old internal APIs or identical contract presentation.

- R47 (requirement/correction): lower preview detail must not merely shrink the
  final display image while retaining full-resolution RCD work. Investigate
  darktable's scaling and reduce image data before RCD. The endpoint-only
  implementation and proposed deferral were assistant choices, not user approval.

- R48 (requirement/correction): the global context passed to every node is meant
  to govern processing such as preview scale. Demosaic performs the requested
  reduction internally; there must be no temporary hidden reduction nodes.
  The earlier injected-DAG approach was the assistant's design, not approval.

- R49 (requirement/correction): question the separate sigmoid preparation node;
  there is no expected independent consumer of its coefficients.

- R50 (requirement/clarification): nodes are ordinary functions with the extra
  machinery needed for a DAG. “Minimum work” means moving repeated graph and
  execution logic into DAG/eval, not splitting algorithms into their finest units.

- R51 (requirement): audit the other parts against that distinction.

- R52 (requirement): expose backend selection through an environment variable,
  suggested `DRIP_BACKEND`; CPU selection is available when `drip/cpu` is enabled.

- R53 (observation/question): some ports show names while others show raw Rust
  types; explain the inconsistency.

- R54 (request): put the display alias together with the `#[node(...)]`
  declaration. `port_labels(...)` is the implementation syntax chosen for this.

- R55 (observation/question): why sigmoid and sigmoid preparation still occupy
  separate files after merging the nodes. The private helper was subsequently
  consolidated into the node module; no preparation node was present.

- R56 (requirement): each node owns a file/directory module. Only shared kernels
  belong in a centralized file; RCD kernels stay with RCD, matrix multiplication
  may live in `shared_kernel.rs`.
- R57 (requirement): in `drip`, node-specific behavior belongs under `node/`;
  the rest contains shared DAG/evaluation infrastructure.
- R58 (requirement): document the organization briefly in `crates/drip/README.md`.
  Apply the same rule to GUI node-specific behavior under `node_ui/`.
- R59 (requirement): remove the repeated parameter-schema lookup from node
  declarations; infer it from the function's parameter type.
- R60 (requirement): `drip_gui::worker=trace` should show per-node timings.
  Tracing must not change computation scheduling or add synchronization; report
  only what can be observed under normal execution.
- R61 (requirement): benchmark the rewrite against the original CPU implementation
  and profile the slowdown. Reproduce release-mode WGPU on the Sony A7R III RAW.
- R62 (requirement): formulate a proposal before making performance fixes.
