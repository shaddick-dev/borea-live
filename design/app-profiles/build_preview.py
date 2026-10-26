"""Build the downloadable, double-clickable preview from the editable canvas source."""
from pathlib import Path
import json
base = Path(__file__).parent
source = (base/'project/Prototype.dc.html').read_text().replace('../logos/', 'logos/')
runtime = (base/'project/preview-runtime.js').read_text()
encoded = json.dumps(source).replace('</', '<\\/')
(base/'index.html').write_text('''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Borea · App profiles preview</title><style>body{margin:0;background:#121c25}#preview{width:1280px;margin:auto}button,input{font-family:inherit}html{color-scheme:dark}</style><body data-start="packs"><div id="preview"></div><script>window.BOREA_PREVIEW_SOURCE=''' + encoded + ';</script><script>' + runtime + '</script></body></html>')
