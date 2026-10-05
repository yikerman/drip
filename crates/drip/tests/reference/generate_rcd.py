"""Reference vectors from upstream RCD, independent of the Rust port.
Run with an optional cached rcd.c; needs Python 3 and a C compiler.
Only interior pixels are compared: Drip deliberately uses bilinear borders.
"""
import pathlib, subprocess, sys, tempfile, urllib.request
REVISION='61dea294bedb3ab6c7cca1a45530b1ab5c0461f3'
URL=f'https://raw.githubusercontent.com/darktable-org/darktable/{REVISION}/src/iop/demosaicing/rcd.c'
s=pathlib.Path(sys.argv[1]).read_text() if len(sys.argv)>1 else urllib.request.urlopen(URL).read().decode()
a=s.index('static void rcd_demosaic(');opening=s.index('{',a);end=opening+1;depth=1
while depth:
 depth+=(s[end]=='{')-(s[end]=='}');end+=1
code='''#include <stdlib.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <math.h>
#define DT_RCD_TILESIZE 148
#define RCD_BORDER 10
#define RCD_MARGIN 9
#define RCD_TILEVALID (DT_RCD_TILESIZE-2*RCD_BORDER)
#define w1 DT_RCD_TILESIZE
#define w2 (2*w1)
#define w3 (3*w1)
#define w4 (4*w1)
#define eps 1e-5f
#define epssq 1e-10f
#define MIN(a,b) ((a)<(b)?(a):(b))
#define MAX(a,b) ((a)>(b)?(a):(b))
#define CLIP(x) fminf(1,fmaxf(0,x))
#define sqrf(x) ((x)*(x))
#define DT_OMP_PRAGMA(...)
#define DT_ALIGNED_PIXEL
#define dt_calloc_align_float(n) calloc(n,sizeof(float))
#define dt_alloc_align_float(n) calloc(n,sizeof(float))
#define dt_free_align free
#define FC(r,c,filters) colors[((r)&1)*2+((c)&1)]
static int colors[4];
static float _safe_in(float a,float scale) {return fmaxf(0,a)*scale;}
static float interpolatef(float t,float a,float b) {return t*a+(1-t)*b;}
static void demosaic_ppg(float *out,const float *in,int w,int h,uint32_t f,float t,int b) {}
'''+s[a:end]+'''
int main(void) {
  const int width=277,height=269;
  const int phases[4][4]={{0,1,1,2},{1,0,2,1},{1,2,0,1},{2,1,1,0}};
  float *in=calloc(width*height,sizeof(float)),*out=calloc(width*height*4,sizeof(float));
  for(int i=0;i<width*height;i++) in[i]=(float)((i*137+(i/width)*29)%2048)/1024.0f-0.03f;
  for(int p=0;p<4;p++) {
    memcpy(colors,phases[p],sizeof(colors));
    rcd_demosaic(out,in,width,height,0,1.0f);
    for(int n=0;n<200;n++) {
      int y=12+(n*53)%245,x=12+(n*79)%253;
      int i=(y*width+x)*4;
      printf("%d,%d,%d,%.9g,%.9g,%.9g\\n",p,y,x,out[i],out[i+1],out[i+2]);
    }
  }
}
'''
with tempfile.TemporaryDirectory() as tmp:
 c=pathlib.Path(tmp)/'reference.c';exe=pathlib.Path(tmp)/'reference';c.write_text(code)
 subprocess.run(['cc','-std=c11','-O0',str(c),'-lm','-o',str(exe)],check=True)
 output=subprocess.check_output([str(exe)],text=True)
pathlib.Path(__file__).with_name('rcd.csv').write_text(f'# darktable {REVISION}; phase,row,col,R,G,B; 277x269 patterned mosaic\n'+output)
