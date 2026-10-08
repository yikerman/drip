#!/usr/bin/env python3
"""Install/remove temporary drivers. Numerical implementations are never patched."""
from pathlib import Path
import argparse
p=argparse.ArgumentParser()
p.add_argument('rayon_checkout',type=Path)
p.add_argument('wgpu_checkout',type=Path)
p.add_argument('--workers',action='store_true')
p.add_argument('--clean',action='store_true')
a=p.parse_args();here=Path(__file__).resolve().parent
marker='\n// BEGIN TEMPORARY PIPELINE BENCHMARK\n'
for root,backend in [(a.rayon_checkout,'rayon'),(a.wgpu_checkout,'wgpu')]:
    root=root.resolve()
    path=root/'crates/drip/examples'/f'compare_{backend}.rs'
    if a.clean: path.unlink(missing_ok=True)
    else:path.write_text(f'mod backend {{ include!("{here}/{backend}.rs"); }}\ninclude!("{here}/harness.rs");\n')
    worker=root/'crates/drip-gui/src/worker.rs'
    source=worker.read_text().split(marker)[0]
    if a.workers and not a.clean:
        source+=marker+f'#[cfg(test)] mod pipeline_benchmark {{ include!("{here}/worker_{backend}.rs"); include!("{here}/worker_bench.rs"); }}\n'
    worker.write_text(source)
transfer=a.wgpu_checkout/'crates/drip/examples/compare_transfer.rs'
if a.clean:transfer.unlink(missing_ok=True)
else:transfer.write_text(f'include!("{here}/transfer.rs");\n')
