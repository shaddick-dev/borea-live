"""Build-time only; emits Rust tables from glibc, xkeyboard-config, tzdata and Babel/CLDR."""
import sys,json,xml.etree.ElementTree as ET,hashlib
from pathlib import Path
from babel import Locale
from babel.core import get_global
r=Path(__file__).resolve().parents[2];out=r/'crates/borea-locale/src/generated.rs'
q=lambda s:json.dumps(s,ensure_ascii=False)
glibc=(r/'crates/borea-locale/data/glibc-SUPPORTED').read_text()
locales=[]
for line in glibc.splitlines():
 words=line.removesuffix('\\').strip().split('/')
 if len(words)!=2 or words[1].strip()!='UTF-8' or words[0].startswith('#'):continue
 key=words[0]
 if '.' not in key:
  key=key.replace('@','.UTF-8@') if '@' in key else key+'.UTF-8'
 base=key.split('.')[0]
 try:
  loc=Locale.parse(base);en=loc.get_display_name('en');native=loc.get_display_name(loc)
 except Exception:en=native=base
 locales.append((key,en,native))
xml=ET.parse('/usr/share/X11/xkb/rules/evdev.xml');keyboards=[]
for layout in xml.findall('./layoutList/layout'):
 conf=layout.find('configItem');key=conf.findtext('name');desc=conf.findtext('description')
 keyboards.append((key,'',desc))
 for variant in layout.findall('./variantList/variant'):
  keyboards.append((key,variant.findtext('configItem/name'),variant.findtext('configItem/description')))
zone=Path('/usr/share/zoneinfo');zones=[]
for p in zone.rglob('*'):
 rel=p.relative_to(zone).as_posix()
 if p.is_file() and rel not in ('localtime','posixrules') and not rel.startswith(('posix/','right/')):
  try:
   if p.read_bytes().startswith(b'TZif'):zones.append(rel)
  except OSError:pass
likely=get_global('likely_subtags');defaults=[]
countryZones={}
for line in (zone/'zone1970.tab').read_text().splitlines():
 if not line or line.startswith('#'):continue
 countries,_,tz,*_=line.split('\t')
 for c in countries.split(','):countryZones.setdefault(c,tz)
console={'us':'us','gb':'uk','de':'de-latin1','fr':'fr','es':'es','it':'it','ru':'ru','br':'br-abnt2','pt':'pt-latin1','se':'sv-latin1','no':'no','dk':'dk-latin1','fi':'fi','jp':'jp106','kr':'us','cn':'us','tw':'us','in':'us'}
console_map={}
for line in (r/'crates/borea-locale/data/kbd-model-map').read_text().splitlines():
 if line.startswith('#'):continue
 fields=line.split()
 if len(fields)>=4 and fields[2] not in console_map:
  console_map[fields[2]]=fields[0]
known_console={line.split()[0] for line in (r/'crates/borea-locale/data/kbd-model-map').read_text().splitlines() if line.strip() and not line.startswith('#')}
locSet={x[0] for x in locales};layoutSet={x[0] for x in keyboards}
for c,tz in sorted(countryZones.items()):
 tag=likely.get('und_'+c,'en_Latn_US').split('_');lang=tag[0];loc=lang+'_'+c+'.UTF-8'
 if loc not in locSet:loc=next((x[0] for x in locales if x[0].startswith(lang+'_')), 'en_US.UTF-8')
 kb=c.lower()
 if kb not in layoutSet:kb='us'
 keymap=console.get(kb,console_map.get(kb,'us'))
 if keymap not in known_console:keymap=console_map.get(kb,'us')
 defaults.append((c,loc,kb,keymap,tz))
assert len(locales)>300, 'glibc input was not parsed completely'
text='// Generated from glibc SUPPORTED, xkeyboard-config evdev.xml, tzdata and CLDR.\n'
text+='pub const LOCALES: &[Locale] = &[\n'+''.join('Locale {id:'+q(a)+',english:'+q(b)+',native:'+q(c)+'},\n' for a,b,c in sorted(set(locales)))+'];\n'
text+='pub const KEYBOARDS: &[KeyboardEntry] = &[\n'+''.join('KeyboardEntry {layout:'+q(a)+',variant:'+q(b)+',description:'+q(c)+'},\n' for a,b,c in keyboards)+'];\n'
text+='pub const TIMEZONES: &[&str] = &[\n'+''.join(q(t)+',\n' for t in sorted(zones))+'];\n'
text+='pub const DEFAULTS: &[CountryDefault] = &[\n'+''.join('CountryDefault {country:'+q(a)+',locale:'+q(b)+',layout:'+q(c)+',console:'+q(d)+',timezone:'+q(e)+'},\n' for a,b,c,d,e in defaults)+'];\n'
out.write_text(text)
(r/'docs/LOCALE-DATA.md').write_text(f'# Locale table provenance\n\nGenerated from glibc SUPPORTED SHA256 {hashlib.sha256(glibc.encode()).hexdigest()},\nhost xkeyboard-config evdev.xml and tzdata zone1970.tab/TZif aliases, Babel CLDR.\n{len(locales)} UTF-8 locales, {len(keyboards)} keyboard layout/variant entries,\n{len(zones)} zones/aliases, {len(defaults)} country suggestions.\nAll tables are compiled into Rust; no runtime XML/CLDR parser or Python is required.\nPython/Babel is a build-only generator dependency. Country defaults are suggestions, not accepted installation choices.\n')
