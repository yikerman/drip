#!/usr/bin/env python3
"""Run already-built release binaries sequentially and retain raw timings."""
import argparse,csv,json,os,subprocess,time
from pathlib import Path
p=argparse.ArgumentParser()
p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[3])
p.add_argument('--repetitions',type=int,default=9)
p.add_argument('--pilot',action='store_true')
a=p.parse_args();root=a.root.resolve();out=Path(__file__).resolve().parent/('pilot' if a.pilot else 'measurements');out.mkdir(exist_ok=True)
fixtures=[('5mp','fixtures/raw/pixls/sony-ilce-7s.arw')]
if not a.pilot:fixtures.append(('42mp','fixtures/raw/sony-ilce-7rm3.arw'))
levels=[0] if a.pilot else [2,1,0]
for fixture,path in fixtures:
 for level in levels:
  for backend,threads,boundary in [('rayon',6,'host'),('rayon',12,'host'),('wgpu',0,'resident'),('wgpu',0,'host')]:
   key=f'{fixture}-level{level}-{backend}-{threads}-{boundary}'
   print(f'RUN {key}',flush=True)
   env=os.environ.copy();env.update(RAYON_NUM_THREADS=str(threads or 12),WGPU_BACKEND='vulkan')
   command=['/usr/bin/time','-f','max_rss_kib=%M major_faults=%F wall_seconds=%e','-o',str(out/f'{key}.resources'),str(root/f'target/release/examples/compare_{backend}'),str(root/path),str(level),str(a.repetitions),boundary,str(out/f'{key}.samples.f32')]
   with (out/f'{key}.csv').open('w') as stdout,(out/f'{key}.log').open('w') as stderr:
    stdout.write('backend,scenario,level,iteration,ms,executed\n');stdout.flush()
    result=subprocess.run(command,env=env,stdout=stdout,stderr=stderr)
   if result.returncode:
    raise SystemExit(f'{key} failed: {(out/f"{key}.log").read_text()}')
   rows=list(csv.DictReader((out/f'{key}.csv').open()))
   import statistics
   for scenario in ['fresh','highlight','exposure','sigmoid','noop']:
    values=[float(r['ms']) for r in rows if r['scenario']==scenario]
    print(f'  {scenario}: {statistics.median(values):.3f} ms',flush=True)
   print((out/f'{key}.resources').read_text().strip(),flush=True)
   time.sleep(0.3)
