# Work remaining

| Work | Status | Scope |
|---|---|---|
| Tiling / large software-adapter images | Deferred | Whole-image processing currently requires each allocation to fit a storage binding; preview downscale does not eliminate the initial RAW upload |
| Host resource budget | Deferred | Decoded sources are cached explicitly; host cache has no byte cap yet |
| User memory-budget setting | Deferred | Library accepts a compute byte budget; frontend currently uses the 2 GiB default |
| Cross-request result cache | Deferred | No node-result cache. Future selection must account for bytes, edit likelihood, execution cost and shared DAG dependencies; use bounded heuristics rather than exact combinatorial search |
| Batch CLI | Deferred | Library and headless examples work; CLI remains a scaffold |
