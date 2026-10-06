from pathlib import Path
import json
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
from fontTools import subset
import argparse
parser=argparse.ArgumentParser(description='Subset pinned Noto source fonts for the embedded UI catalogues')
parser.add_argument('sources', type=Path, help='Directory containing sc.ttf, tc.ttf, jp.ttf, kr.ttf, ar.ttf, he.ttf and their CODE-OFL.txt files')
args=parser.parse_args()
root=Path(__file__).resolve().parent.parent;dst=root/'assets/fonts/interface';dst.mkdir(parents=True,exist_ok=True)
texts=[]
for path in (root/'crates/app/locales').glob('*.json'):
 v=json.loads(path.read_text());texts+=list(v.values()) if isinstance(v,dict) else v
texts.append((root/'crates/app/src/i18n.rs').read_text())
chars=set(ord(c) for text in texts if isinstance(text,str) for c in text)
chars.update(range(0xFB50,0xFE00));chars.update(range(0xFE70,0xFF00))
for code in ['sc','tc','jp','kr','ar','he']:
 import hashlib
 manifest=json.loads((dst/'sources.json').read_text())
 assert hashlib.sha256((args.sources / f'{code}.ttf').read_bytes()).hexdigest() == manifest[code]['sha256'], f'Unexpected {code} source font'
 font=TTFont(args.sources / f'{code}.ttf')
 if 'fvar' in font:font=instantiateVariableFont(font,{a.axisTag:(400 if a.axisTag=='wght' else a.defaultValue) for a in font['fvar'].axes},inplace=True)
 options=subset.Options();options.name_IDs=['*'];options.name_legacy=True;options.name_languages=['*'];options.recalc_timestamp=False
 sub=subset.Subsetter(options=options);sub.populate(unicodes=chars);sub.subset(font)
 # Modified subsets have their own family/PostScript names.
 for record in font['name'].names:
  if record.nameID in (1, 2, 3, 4, 6, 16, 17):
   names={1:f'ActionLay Interface {code}',2:'Regular',3:f'ActionLayInterface-{code}-Regular',4:f'ActionLay Interface {code} Regular',6:f'ActionLayInterface-{code}-Regular',16:f'ActionLay Interface {code}',17:'Regular'}
   record.string=names[record.nameID].encode(record.getEncoding())
 font.save(dst/f'NotoSans-{code}.ttf')
 (dst/f'{code}-OFL.txt').write_bytes((args.sources / f'{code}-OFL.txt').read_bytes())
 print(code,(dst/f'NotoSans-{code}.ttf').stat().st_size,flush=True)
