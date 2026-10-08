#!/usr/bin/env python3
"""Summarize raw measurements; draw medians and observed 10–90% intervals."""
import csv,json,statistics,os
from pathlib import Path
os.environ.setdefault('MPLCONFIGDIR','/tmp/drip-benchmark-matplotlib')
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
here=Path(__file__).resolve().parent
summary=[]; lookup={}; validation=[]
for path in sorted((here/'measurements').glob('*.csv')):
    fixture,level,backend,threads,boundary=path.stem.split('-')
    rows=list(csv.DictReader(path.open()))
    for scenario in dict.fromkeys(r['scenario'] for r in rows):
        selected=[r for r in rows if r['scenario']==scenario]
        values=np.array([float(r['ms']) for r in selected])
        row=dict(fixture=fixture,level=int(level[5:]),backend=backend,threads=int(threads),boundary=boundary,scenario=scenario,n=len(values),median_ms=float(np.median(values)),p10_ms=float(np.quantile(values,.1)),p90_ms=float(np.quantile(values,.9)),min_ms=float(min(values)),max_ms=float(max(values)),executed_nodes=sorted(set(int(r['executed']) for r in selected)))
        summary.append(row);lookup[(fixture,int(level[5:]),backend,int(threads),boundary,scenario)]=row
    if backend=='wgpu':
        baseline=path.with_name(f'{fixture}-{level}-rayon-12-host.samples.f32')
        if baseline.exists():
            a=np.fromfile(baseline,dtype='<f4').astype(float)
            b=np.fromfile(path.with_suffix('.samples.f32'),dtype='<f4').astype(float)
            assert len(a)==len(b)
            d=a-b
            validation.append(dict(case=path.stem,channels=len(a),max_abs_error=float(max(abs(d))),rmse=float(np.sqrt(np.mean(d*d)))))
(here/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
if validation:
    (here/'numerical-check.json').write_text(json.dumps(validation,indent=2)+'\n')
else:
    validation=json.loads((here/'numerical-check.json').read_text())
with (here/'summary.csv').open('w') as f:
    w=csv.DictWriter(f,fieldnames=list(summary[0]));w.writeheader();w.writerows(summary)
plt.rcParams.update({'font.family':'DejaVu Sans','font.size':10,'axes.spines.top':False,'axes.spines.right':False})
fig,axes=plt.subplots(2,2,figsize=(12,8.8))
scenarios=['fresh','highlight','exposure','sigmoid']
labels=['Recompute all','Edit highlights','Edit exposure','Edit sigmoid']
colors=['#696969','#2377b9','#c05a27']
series=[('rayon',12,'host','Rayon, 12 threads'),('wgpu',0,'resident','wgpu, keep output on GPU'),('wgpu',0,'host','wgpu, download output to CPU')]
for ax,(fixture,level,title) in zip(axes.flat,[('5mp',0,'5.2 MP input → 5.2 MP output'),('42mp',0,'42.4 MP input → 42.4 MP output'),('5mp',2,'5.2 MP input → 0.33 MP preview'),('42mp',2,'42.4 MP input → 2.65 MP preview')]):
    limit=max(lookup[fixture,level,b,t,m,s]['p90_ms'] for b,t,m,_ in series for s in scenarios)*1.2
    for j,(backend,threads,boundary,name) in enumerate(series):
        selected=[lookup[fixture,level,backend,threads,boundary,s] for s in scenarios]
        x=np.array([r['median_ms'] for r in selected]);lo=np.array([r['p10_ms'] for r in selected]);hi=np.array([r['p90_ms'] for r in selected]);y=np.arange(4)+(j-1)*.24
        ax.barh(y,x,height=.20,color=colors[j],label=name,xerr=[x-lo,hi-x],error_kw={'lw':.8,'capsize':2,'ecolor':'#222'})
        for yi,xi,right in zip(y,x,hi):ax.text(max(xi,right)+limit*.015,yi,f'{xi:.1f}',va='center',fontsize=9)
    ax.set_title(title,loc='left',fontweight='bold');ax.set_yticks(range(4),labels);ax.invert_yaxis();ax.set_xlim(0,limit);ax.set_xlabel('Latency (ms; lower is faster)');ax.grid(axis='x',alpha=.18);ax.set_axisbelow(True)
handles,names=axes[0,0].get_legend_handles_labels();fig.legend(handles,names,loc='upper center',ncol=3,bbox_to_anchor=(.5,.955),frameon=False)
fig.suptitle('The GPU wins full recomputation; readback and lost cache reuse reduce the gain',fontsize=14,x=.05,ha='left',y=.99)
fig.text(.05,.018,'Median of 9 sequential release runs, after 2 warmups; whiskers show the 10th–90th percentiles.\nSame preloaded RAW and pipeline. Old Rayon keeps its upstream cache during edits; current wgpu recomputes all nodes.',fontsize=9)
fig.tight_layout(rect=(0,.065,1,.91));fig.savefig(here/'latency.png',dpi=160);fig.savefig(here/'latency.svg');plt.close(fig)
for level in [2,1,0]:
    print(f'42mp level{level}')
    for scenario in scenarios:
        rows=[lookup['42mp',level,b,t,m,scenario]['median_ms'] for b,t,m,_ in series]
        print(scenario, *(round(v,2) for v in rows),'resident speedup',round(rows[0]/rows[1],2),'readback speedup',round(rows[0]/rows[2],2))
print('Max sampled error:',max(r['max_abs_error'] for r in validation))

# The actual worker includes scopes and preview preparation, unlike the headless plot.
worker_rows=[]
for path in sorted((here/'workers').glob('*.csv')):
    rows=list(csv.DictReader(path.open()))
    for scenario in ['highlight','exposure','sigmoid','noop']:
        selected=[r for r in rows if r['scenario']==scenario]
        values=np.array([float(r['ms']) for r in selected])
        worker_rows.append(dict(backend=selected[0]['backend'],level=int(selected[0]['level']),scenario=scenario,n=len(values),median_ms=float(np.median(values)),p10_ms=float(np.quantile(values,.1)),p90_ms=float(np.quantile(values,.9))))
if worker_rows:
    (here/'worker-summary.json').write_text(json.dumps(worker_rows,indent=2)+'\n')
    fig,axes=plt.subplots(1,2,figsize=(11,4.5))
    for ax,level,title in zip(axes,[1,2],['Half-size preview: 10.6 MP (default)','Quarter-size preview: 2.65 MP']):
        for j,(backend,name,color) in enumerate([('rayon-worker','Old Rayon, 12 threads','#696969'),('wgpu-worker','Current wgpu','#2377b9')]):
            rows=[next(r for r in worker_rows if r['level']==level and r['backend']==backend and r['scenario']==s) for s in ['highlight','exposure','sigmoid']]
            x=np.array([r['median_ms'] for r in rows]);lo=np.array([r['p10_ms'] for r in rows]);hi=np.array([r['p90_ms'] for r in rows]);y=np.arange(3)+(j-.5)*.30
            ax.barh(y,x,height=.25,color=color,label=name,xerr=[x-lo,hi-x],error_kw={'lw':.8,'capsize':2,'ecolor':'#222'})
            for yi,xi,right in zip(y,x,hi):ax.text(max(xi,right)+8,yi,f'{xi:.0f}',va='center',fontsize=10)
        ax.set_xlim(0,max(r['p90_ms'] for r in worker_rows if r['level']==level and r['scenario']!='noop')*1.14);ax.set_yticks(range(3),['Edit highlights','Edit exposure','Edit sigmoid']);ax.invert_yaxis();ax.set_title(title,loc='left',fontweight='bold');ax.set_xlabel('Worker latency (ms; lower is faster)');ax.grid(axis='x',alpha=.18);ax.set_axisbelow(True)
    fig.suptitle('Actual preview worker: gains for early edits, regressions for late edits',x=.03,ha='left',fontsize=14)
    handles,names=axes[0].get_legend_handles_labels();fig.legend(handles,names,ncol=2,loc='upper center',bbox_to_anchor=(.5,.925),frameon=False)
    fig.text(.03,.018,'42.4 MP RAW; preview + histogram + waveform + vectorscope. Window drawing excluded.\nMedian of 9 sequential release runs; whiskers show the 10th–90th percentiles.',fontsize=9)
    fig.tight_layout(rect=(0,.075,1,.85));fig.savefig(here/'worker-latency.png',dpi=160);fig.savefig(here/'worker-latency.svg');plt.close(fig)
