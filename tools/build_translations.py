#!/usr/bin/env python3
"""Build the four shipped catalogs and report untranslated resource text."""
import json,re,struct
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
LOCALES=ROOT/'crates/gui/assets/locales'
CATALOG=json.loads((LOCALES/'messages.json').read_text()) if (LOCALES/'messages.json').exists() else {}
for number,line in enumerate((LOCALES/'glossary.tsv').read_text().splitlines(),1):
 if not line.strip() or line.startswith('#'):continue
 cells=[cell.replace('\\n','\n') for cell in line.split('\t')]
 if len(cells)==4:cells.append(cells[-1])
 if len(cells)!=5:raise ValueError(f'glossary line {number}: expected English and four translations')
 CATALOG[cells[0]]=cells[1:]
PROPER={'Impress','Typst','A4','APA','Chicago','IEEE','MLA','Letter','Linux Libertine G','Times New Roman','Cambria','Georgia','Calibri','Arial','Segoe UI','Consolas','Latin Modern Math','SVG','PDF','PNG','JPEG','DOCX','RTF','HTML','RGB','CMYK','Ctrl','Shift','Alt','Enter','Tab','Esc','F7','F11','BCP 47','DIP','OCR','OpenType','Harvard','A5','Legal','cm','mm','pt','document.pdf'}
TECHNICAL=re.compile(r'^(?:[\d\W]+|[A-Z]|(?:\d+(?:[.,½¼]\d+)?\s*(?:pt|mm|cm|%|°|dpi))|(?:\d+:)|(?:Col #))$')

def derive(key):
 if key in CATALOG:return CATALOG[key]
 if key in PROPER or TECHNICAL.fullmatch(key) or re.fullmatch(r'[\d¼½¾][\d¼½¾\s_]*pt|4 _______',key):return [key]*4
 if key.endswith(' ribbon') and key[:-7] in CATALOG:
  row=CATALOG[key[:-7]]
  return ['Κορδέλα '+row[0],'Cinta '+row[1],'Faixa de opções '+row[2],'Friso '+row[3]]
 match=re.fullmatch(r'Level (\d+)',key)
 if match:return [v+' '+match[1] for v in CATALOG['Level']]
 match=re.fullmatch(r'(\d+) pt glow',key)
 if match:return [v.replace('{number}',match[1]) for v in CATALOG['{number} pt glow']]
 # Captions retain their punctuation and keyboard hints around translated text.
 for left,right in [('&',''),('','…'),('','...'),('',':'),('', ' ▾')]:
  if (not left or key.startswith(left)) and (not right or key.endswith(right)):
   inner=key[len(left):len(key)-len(right) if right else None]
   row=derive(inner)
   if row is not None:return [v+right for v in row]
 without=key.replace('&','')
 if without in CATALOG:return CATALOG[without]
 match=re.fullmatch(r'(.+?)( \([^()]+\))',key)
 if match and match[1] in CATALOG:return [v+match[2] for v in CATALOG[match[1]]]
 suffix=' — Not available in this alpha'
 if key.endswith(suffix) and 'Not available in this alpha' in CATALOG:
  base=derive(key[:-len(suffix)])
  if base:return [a+' — '+b for a,b in zip(base,CATALOG['Not available in this alpha'])]
 return None
# Runtime-built dialogs also use punctuated variants of glossary captions.
# Extract literals for catalog derivation only; authored document data is never
# sent through the localization boundary.
for source in (ROOT/'crates/gui/src').rglob('*.rs'):
 if source.name == 'widgets.rs':continue
 for literal in re.findall(r'"(?:[^"\\]|\\.)*"',source.read_text()):
  try:key=json.loads(literal)
  except (ValueError, TypeError):continue
  row=derive(key)
  if row is not None and row!=[key]*4:CATALOG[key]=row
resources=json.loads((LOCALES/'resource-strings.json').read_text())
missing=[]
for key in resources:
 row=derive(key)
 if row is None:missing.append(key)
 else:CATALOG[key]=row
from collections import Counter
for key,row in CATALOG.items():
 if len(row)!=4 or any(not value for value in row):raise ValueError(f'Incomplete translation: {key!r}')
 expected=Counter(re.findall(r'\{[^{}]*\}',key))
 for value in row:
  if Counter(re.findall(r'\{[^{}]*\}',value))!=expected:raise ValueError(f'Changed format placeholders: {key!r}: {value!r}')
(LOCALES/'translations.json').write_text(json.dumps(CATALOG,ensure_ascii=False,indent=2,sort_keys=True)+'\n')
if missing:print('Missing resource strings:\n'+'\n'.join(missing))
print(f'{len(CATALOG)} catalog entries; {len(missing)} resource strings need translation.')

# Native wxWidgets stock labels and XRC resources use the same embedded catalog.
def mo_bytes(language,column):
 pairs={'':'Content-Type: text/plain; charset=UTF-8\nLanguage: '+language+'\n'}
 for key,row in CATALOG.items():
  pairs[key]=row[column]
  if key in ['OK','Cancel','Yes','No','Apply','Close','Help','Save','Open']:
   pairs['&'+key]=row[column]
 keys=sorted(pairs);original=[key.encode() for key in keys];translated=[pairs[key].encode() for key in keys]
 count=len(keys);base=28+count*16;first=b'';second=b'';original_table=[];translated_table=[]
 for value in original:original_table.append((len(value),base+len(first)));first+=value+b'\0'
 for value in translated:translated_table.append((len(value),base+len(first)+len(second)));second+=value+b'\0'
 header=struct.pack('<7I',0x950412de,0,count,28,28+count*8,0,0)
 tables=b''.join(struct.pack('<2I',*entry) for entry in original_table+translated_table)
 return header+tables+first+second
for index,language in enumerate(['el','es','pt_BR','pt_PT']):
 (LOCALES/(language+'.mo')).write_bytes(mo_bytes(language,index))
