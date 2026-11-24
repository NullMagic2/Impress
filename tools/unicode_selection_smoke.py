"""Native GTK Unicode selection regression using an imported DOCX fixture.

Run with xdotool, setxkbmap, xfwm4 and Pillow installed, on a disposable X server:
  dbus-run-session -- xvfb-run -a -s '-screen 0 1600x1150x24' python3 tools/unicode_selection_smoke.py \
      /absolute/path/to/Impress --output /tmp/impress-keyboard-results

Checks saved document text after every selection/replacement and captures
the native selection highlight. Never use the user's desktop X server: this changes
its keyboard layout. Windows/IMM32 needs separate native verification.
"""
from pathlib import Path
import json, os, subprocess, sys, tempfile, time, zipfile
import xml.etree.ElementTree as ET
import argparse
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('executable',type=Path)
parser.add_argument('--output',type=Path,required=True)
args=parser.parse_args()
exe=args.executable.resolve()
qa=args.output.resolve();qa.mkdir(parents=True,exist_ok=True)
mode='keyboard'
def x(*args):
    return subprocess.check_output(['xdotool',*map(str,args)],text=True,timeout=10).strip()
def key(*keys):
    x('key',*keys);time.sleep(.3)
def click(where):
    x('mousemove',*where,'click',1);time.sleep(.3)
def wait_for_window(name):
    deadline=time.monotonic()+30
    while time.monotonic()<deadline:
        if wm.poll() is not None:
            raise AssertionError('Window manager exited; see keyboard-wm.log')
        if app.poll() is not None:
            raise AssertionError('Impress exited; see keyboard-gui.log')
        result=subprocess.run(['xdotool','search','--onlyvisible','--class','^Impress$'],capture_output=True,text=True)
        if result.returncode==0 and result.stdout.strip():
            # GTK publishes the UTF-8 title through _NET_WM_NAME; older
            # xdotool search --name only checks the legacy WM_NAME.
            for window in result.stdout.strip().splitlines():
                if name in x('getwindowname',window):
                    x('windowactivate','--sync',window)
                    return
        time.sleep(.1)
    screenshot().save(qa/'keyboard-startup.png')
    raise AssertionError('Impress did not open the synthetic keyboard fixture')
def screenshot():
    from PIL import ImageGrab
    return ImageGrab.grab(xdisplay=os.environ['DISPLAY'])
ns={'w':'http://schemas.openxmlformats.org/wordprocessingml/2006/main'}
def text(path):
 try:
  with zipfile.ZipFile(path) as z:
   r=ET.fromstring(z.read('word/document.xml'))
   return '\n'.join(''.join(t.text or '' for t in p.findall('.//w:t',ns)) for p in r.findall('.//w:body/w:p',ns))
 except (zipfile.BadZipFile,FileNotFoundError):return None
cases=[('Greek repeated quotes','gr','όλα'+'"'*100, '"'),
       ('Greek composed accent','gr','όλαά','ά'),
       ('Greek decomposed accents','gr','όλαι\u0308\u0301','ι\u0308\u0301'),
       ('Latin decomposed accent','us','cafe\u0301','e\u0301'),
       ('Curly quotes','us','word“”','”'),
       ('Emoji sequence','us','word👩\u200d💻','👩\u200d💻')]
originals=[c[2] for c in cases]+['αβγδεζηθ'+'"'*30+' ending']
with tempfile.TemporaryDirectory(prefix='impress-keyboard-') as tmp:
 tmp=Path(tmp)
 env={**os.environ,'LANGUAGE':'en','GTK_IM_MODULE':'simple','GDK_BACKEND':'x11','GTK_USE_PORTAL':'0'}
 for v in ['XDG_CONFIG_HOME','XDG_CACHE_HOME','XDG_DATA_HOME','IMPRESS_RECOVERY_DIR']:env[v]=str(tmp/v)
 path=tmp/'Keyboard.docx'
 with zipfile.ZipFile(path,'w',zipfile.ZIP_DEFLATED) as z:
  z.writestr('[Content_Types].xml','<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>')
  z.writestr('_rels/.rels','<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>')
  from xml.sax.saxutils import escape
  paragraphs=''.join('<w:p><w:r><w:t>'+escape(t)+'</w:t></w:r></w:p>' for t in originals)
  z.writestr('word/document.xml','<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>'+paragraphs+'<w:sectPr/></w:body></w:document>')
 wm_log=(qa/'keyboard-wm.log').open('w')
 wm=subprocess.Popen(['xfwm4','--compositor=off'],stdout=wm_log,stderr=subprocess.STDOUT)
 time.sleep(.5)
 log=(qa/(mode+'-gui.log')).open('w')
 app=subprocess.Popen([exe,path],env=env,stdout=log,stderr=subprocess.STDOUT)
 report=[]
 try:
  wait_for_window('Keyboard.docx');key('alt+F10');time.sleep(.7)
  click((800,530));key('ctrl+Home')
  def save():
   # Force a save even when the idle document is clean, preserving its text.
   key('space');key('BackSpace')
   before=path.stat().st_mtime_ns;key('ctrl+s');deadline=time.monotonic()+8
   while time.monotonic()<deadline:
    if path.stat().st_mtime_ns!=before and text(path) is not None:return text(path)
    time.sleep(.1)
   screenshot().save(qa/(mode+'-save.png'))
   raise AssertionError('Save did not complete')
  def layout(lang,variant=''):
   subprocess.run(['setxkbmap','-layout',lang,'-variant',variant,'-option',''],check=True);time.sleep(.15)
  expected=list(originals)
  def paragraph_end(index):
   layout('us');key('ctrl+a');key('Left')
   for _ in range(index):key('ctrl+Down')
   key('End')
  for index,(name,lang,original,grapheme) in enumerate(cases):
   paragraph_end(index);layout(lang);key('shift+Left')
   screenshot().save(qa/(name.replace(' ','-')+'-selected.png'))
   key('BackSpace');layout('us');key('x');time.sleep(.5)
   actual=save().split('\n');expected[index]=original[:-len(grapheme)]+'x'
   report.append({'case':name,'expected':expected[index],'actual':actual[index],'pass':actual==expected})
   print(json.dumps(report[-1],ensure_ascii=False),flush=True)
  paragraph_end(len(cases))
  x('keydown','Control_L','keydown','Shift_L','keydown','Left')
  time.sleep(.08);x('keyup','Control_L');time.sleep(.45)
  x('keyup','Left','keyup','Shift_L');time.sleep(.2)
  screenshot().save(qa/'released-control-selected.png')
  key('BackSpace');key('x');time.sleep(.5)
  actual=save().split('\n')
  report.append({'case':'Released Ctrl during Shift+arrow hold','actual':actual[-1],
                 'pass':actual[:-1]==expected[:-1] and actual[-1].startswith('αβγδεζηθ"') and actual[-1].endswith('x') and len(actual[-1])<len(originals[-1])})
  print(json.dumps(report[-1],ensure_ascii=False),flush=True)
  (qa/'selection-results.json').write_text(json.dumps(report,indent=2,ensure_ascii=False)+'\n')
 finally:
  app.terminate();app.wait(timeout=10);wm.terminate();wm.wait(timeout=10);log.close();wm_log.close()
if not all(r['pass'] for r in report):sys.exit(1)
