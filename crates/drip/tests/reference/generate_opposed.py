"""Generate reference samples using pinned darktable inpaint-opposed C.
Pass a directory containing opposed.c and segbased.c, or download on demand.
GUI/cache hooks are inert; the upstream pixel algorithm is extracted unchanged.
"""
import pathlib, subprocess, sys, tempfile, urllib.request
REVISION='61dea294bedb3ab6c7cca1a45530b1ab5c0461f3'
def read(name):
 if len(sys.argv)>1:return (pathlib.Path(sys.argv[1])/name).read_text()
 return urllib.request.urlopen(f'https://raw.githubusercontent.com/darktable-org/darktable/{REVISION}/src/iop/hlreconstruct/{name}').read().decode()
def function(source,signature):
 a=source.index(signature);opening=source.index('{',a);end=opening+1;depth=1
 while depth:
  depth+=(source[end]=='{')-(source[end]=='}');end+=1
 return source[a:end]
s=read('opposed.c');ref=read('segbased.c')
code='''#include <stdlib.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <math.h>
#define TRUE 1
#define FALSE 0
#define MIN(a,b) ((a)<(b)?(a):(b))
#define MAX(a,b) ((a)>(b)?(a):(b))
#define DT_OMP_FOR(...)
#define for_each_channel(c) for(int c=0;c<3;c++)
#define for_three_channels(c) for(int c=0;c<3;c++)
#define dt_calloc_aligned(n) calloc(n,1)
#define dt_alloc_align_float(n) calloc(n,sizeof(float))
#define dt_free_align free
#define dt_round_size(n,a) (((n)+(a)-1)/(a)*(a))
#define dt_print_pipe(...)
#define dt_iop_gui_enter_critical_section(...)
#define dt_iop_gui_leave_critical_section(...)
#define dt_iop_copy_image_roi(...)
#define _opposed_hash(...) 0
#define dt_pipe_is_full(...) 0
#define fcol(r,c,filters,xtrans) colors[((r)&1)*2+((c)&1)]
static int colors[4];
static float fcube(float x) {return x*x*x;}
typedef int gboolean;
typedef int dt_hash_t;
typedef float dt_aligned_pixel_t[4];
typedef struct {int width,height,x,y;} dt_iop_roi_t;
typedef struct {struct {int enabled;float coeffs[4];} temperature;} dt_iop_buffer_dsc_t;
typedef struct {int late_correction;float D65coeffs[4],as_shot[4];} dt_dev_chroma_t;
typedef struct {int opphash,oppclipped,oppresult;float oppchroma[4];} dt_iop_highlights_gui_data_t;
typedef struct {int gui_attached;dt_dev_chroma_t chroma;} Dev;
typedef struct {dt_iop_buffer_dsc_t dsc;int devid;} Pipe;
typedef struct {void *gui_data;Dev *dev;} dt_iop_module_t;
typedef struct {Pipe *pipe;uint8_t xtrans[6][6];uint32_t filters;} dt_dev_pixelpipe_iop_t;
'''
code+=function(ref,'static inline float _calc_refavg(')
for sig in ['static inline size_t _raw_to_cmap(','static inline char _mask_dilated(','static float *_process_opposed(']:code+=function(s,sig)+'\n'
code+='''
int main(void) {
 const int w=96,h=90;
 float *in=calloc(w*h,sizeof(float)),*out=calloc(w*h,sizeof(float));
 const int phases[4][4]={{0,1,1,2},{1,0,2,1},{1,2,0,1},{2,1,1,0}};
 const float gains[3]={1.3f,1.0f,1.7f};
 Dev dev={0};Pipe pipe={0};dt_iop_module_t self={NULL,&dev};dt_dev_pixelpipe_iop_t piece={.pipe=&pipe};
 pipe.dsc.temperature.enabled=1;memcpy(pipe.dsc.temperature.coeffs,gains,sizeof(gains));
 dt_iop_roi_t roi={w,h,0,0};
 for(int phase=0;phase<4;phase++) {
   memcpy(colors,phases[phase],sizeof(colors));
   for(int r=0;r<h;r++) for(int c=0;c<w;c++) {
     int ch=colors[(r%2)*2+c%2];
     float v=0.35f+(float)((r*13+c*7)%31)/100.0f;
     if(r>=32&&r<58&&c>=32&&c<64) v=ch==0?1.0f:0.9f;
     in[r*w+c]=v*gains[ch];
   }
   _process_opposed(&self,&piece,in,out,&roi,&roi,FALSE,0.98f);
   for(int r=28;r<62;r+=2) for(int c=28;c<68;c+=3)
     printf("%d,%d,%d,%.9g\\n",phase,r,c,out[r*w+c]);
 }
}
'''
with tempfile.TemporaryDirectory() as tmp:
 c=pathlib.Path(tmp)/'reference.c';exe=pathlib.Path(tmp)/'reference';c.write_text(code)
 subprocess.run(['cc','-std=c11','-O0',str(c),'-lm','-o',str(exe)],check=True)
 output=subprocess.check_output([str(exe)],text=True)
pathlib.Path(__file__).with_name('opposed.csv').write_text(f'# darktable {REVISION}; phase,row,col,value; 96x90 mosaic\n'+output)
