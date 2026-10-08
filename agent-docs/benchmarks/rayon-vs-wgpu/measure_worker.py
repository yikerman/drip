#!/usr/bin/env python3
"""Time actual preview worker preparation, including histogram/waveform/vectorscope."""
import os,subprocess,csv,statistics
from pathlib import Path
here=Path(__file__).resolve().parent;root=here.parents[2];out=here/'workers';out.mkdir(exist_ok=True)
for level in [2,1]:
 for backend in ['rayon','wgpu']:
    key=f'42mp-level{level}-{backend}'
    env=os.environ.copy();env.update(RAYON_NUM_THREADS='12',WGPU_BACKEND='vulkan',DRIP_BENCH_RAW=str(root/'fixtures/raw/sony-ilce-7rm3.arw'),DRIP_BENCH_LEVEL=str(level),DRIP_BENCH_REPETITIONS='9')
    command=['/usr/bin/time','-f','max_rss_kib=%M major_faults=%F wall_seconds=%e','-o',str(out/f'{key}.resources'),str(root/f'target/release/compare_worker_{backend}'),'pipeline_benchmark::compare_actual_worker','--ignored','--nocapture','--test-threads=1']
    print('RUN '+key,flush=True)
    result=subprocess.run(command,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
    (out/f'{key}.log').write_text(result.stdout)
    if result.returncode:raise SystemExit(result.stdout)
    lines=[line[line.index('BENCH,')+6:] for line in result.stdout.splitlines() if 'BENCH,' in line]
    (out/f'{key}.csv').write_text('backend,scenario,level,iteration,ms\n'+'\n'.join(lines)+'\n')
    rows=list(csv.DictReader((out/f'{key}.csv').open()))
    for scenario in ['highlight','exposure','sigmoid','noop']:
        values=[float(r['ms']) for r in rows if r['scenario']==scenario]
        assert len(values)==9
        print(f'  {scenario}: {statistics.median(values):.3f} ms',flush=True)
