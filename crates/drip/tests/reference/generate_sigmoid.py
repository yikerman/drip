"""Regenerate hue-correction vectors with the pinned upstream C implementation.
Run with Python 3 and a C compiler; takes an optional cached sigmoid.c path.
The extracted GPL-3.0-or-later code is compiled only in a temporary directory.
"""
import pathlib
import subprocess
import sys
import tempfile
import urllib.request

REVISION = '61dea294bedb3ab6c7cca1a45530b1ab5c0461f3'
URL = f'https://raw.githubusercontent.com/darktable-org/darktable/{REVISION}/src/iop/sigmoid.c'
source = pathlib.Path(sys.argv[1]).read_text() if len(sys.argv) > 1 else urllib.request.urlopen(URL).read().decode()

def function(name):
    start = source.index('static inline void ' + name)
    opening = source.index('{', start)
    depth, end = 1, opening + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[start:end]

code = '''#include <stdio.h>
#include <stddef.h>
typedef float dt_aligned_pixel_t[4];
typedef struct { size_t min, mid, max; } dt_iop_sigmoid_value_order_t;
'''+function('_preserve_hue_and_energy')+'''
int main(void) {
  const float values[][3] = {{0,0,0},{1,1,1},{0.1,0.4,2},{2,0.1,0.4},{0.4,2,0.1},{0,0.4,2},{2,0.4,0},{0.4,0,2},{0.1,2,0.4},{0.4,0.1,2},{2,0.4,0.1}};
  for(int n=0;n<11;n++) for(int h=0;h<3;h++) {
    dt_aligned_pixel_t input={values[n][0],values[n][1],values[n][2],0}, mapped={0}, output={0};
    int order[3]={0,1,2};
    for(int i=0;i<3;i++) for(int j=i+1;j<3;j++) if(input[order[i]]>input[order[j]]) {int t=order[i];order[i]=order[j];order[j]=t;}
    for(int c=0;c<3;c++) mapped[c]=input[c]/(input[c]+1);
    dt_iop_sigmoid_value_order_t o={order[0],order[1],order[2]};
    _preserve_hue_and_energy(input,mapped,output,o,h*0.5f);
    for(int c=0;c<3;c++) printf("%.9g,",input[c]);
    for(int c=0;c<3;c++) printf("%.9g,",mapped[c]);
    printf("%.9g,%.9g,%.9g,%.9g\\n",h*0.5f,output[0],output[1],output[2]);
  }
}
'''
with tempfile.TemporaryDirectory() as tmp:
    c, exe = pathlib.Path(tmp)/'reference.c', pathlib.Path(tmp)/'reference'
    c.write_text(code)
    subprocess.run(['cc','-std=c11','-O0',str(c),'-o',str(exe)],check=True)
    output = subprocess.check_output([str(exe)],text=True)
pathlib.Path(__file__).with_name('sigmoid_hue.csv').write_text(f'# darktable {REVISION}; input RGB, mapped RGB, hue, output RGB\n'+output)
