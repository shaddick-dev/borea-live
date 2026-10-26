"""Browser checks for the latest uploaded catalog/profile flow. Run local server on 8766."""
import json
from pathlib import Path
from playwright.sync_api import sync_playwright
root=Path(__file__).parent
with sync_playwright() as p:
 browser=p.chromium.launch(executable_path='/usr/bin/chromium',args=['--no-sandbox'])
 page=browser.new_page(viewport={'width':1280,'height':800})
 page.set_default_timeout(6000)
 errors=[]
 page.on('pageerror',lambda error:errors.append(str(error)))
 page.route('https://fonts.googleapis.com/**',lambda route:route.abort())
 page.goto('http://127.0.0.1:8766',wait_until='domcontentloaded')
 def b(name,**kw):
  import re
  if name=='GamingGames':name=re.compile(r'^Gaming\s*Games')
  return page.get_by_role('button',name=name,**kw)
 def setstate(patch):page.evaluate('(p)=>boreaPreview.setState(p)',patch)
 def v(key):return page.evaluate('(key)=>boreaPreview.renderVals()[key]',key)
 def count(n):assert v('appCount')==str(n),(v('appCount'),n)
 def shot(name):page.screenshot(path=str(root/'previews'/name))
 assert not v('hasEnvironment') and not v('canNext')
 shot('01-system.png')
 b('Workstation').click();assert v('hasEnvironment')
 b('continue').click();assert v('appStage') and not v('canNext')
 b('No extra apps').click();count(0);assert v('canNext')
 b('Everyday').click();count(7)
 b('GamingGames').click();count(12)
 assert 'Gaming Tweaks' in v('addonShort')
 b('Developer').click();count(14)
 assert 'Development Tools' in v('addonShort')
 shot('02-apps.png')
 # Removing one selected app creates an override, not a frozen copy of every preset.
 b('Customize apps').click();page.get_by_role('searchbox').fill('Firefox')
 b('Firefox').click();count(13)
 page.get_by_role('searchbox').fill('no-app-with-this-name');assert page.get_by_text('No apps match.',exact=False).count()==1
 page.get_by_role('searchbox').fill('');shot('03-customize.png')
 page.keyboard.press('Escape');assert page.get_by_role('dialog').count()==0
 b('GamingGames').click();count(8)
 assert 'Gaming Tweaks' not in v('addonShort')
 b('Developer').click();count(6)
 assert 'Development Tools' not in v('addonShort')
 b('Customize apps').click();b('Restore profile apps').click();count(7);b('Done',exact=True).click()
 # Roles may bring an app on desktops; counts/review use that same app.
 b('← back',exact=True).click();b('Add roles & tools').click();b('File Sharing').click();b('Done',exact=True).click();b('continue').click();count(8)
 b('Review all 8 apps').click();assert page.locator('.app-row').count()==8;assert page.get_by_role('button',name='LocalSend',exact=False).is_disabled();page.keyboard.press('Escape')
 b('← back',exact=True).click();assert v('profileStage')
 b('Customize packages').click();assert page.get_by_role('dialog').count()==1
 assert page.get_by_role('tree',name='What to install').count()==1
 shot('04-packages.png')
 before=v('sysCount')
 # The selected environment's optional edits affect the same system package list.
 entry=page.evaluate("boreaPreview.renderVals().tree.find(r=>r.label==='Workstation')")
 page.evaluate("boreaPreview.renderVals().tree.find(r=>r.label==='Workstation').toggle()")
 row=page.evaluate("boreaPreview.renderVals().tree.find(r=>r.level==='3'&&!r.inert&&r.checked==='true')?.label")
 assert row
 page.evaluate("label=>boreaPreview.renderVals().tree.find(r=>r.level==='3'&&r.label===label&&!r.inert).click()",row)
 assert int(v('sysCount'))==int(before)-1
 page.keyboard.press('Escape')
 # Changing profile to headless skips the app page without losing dormant choices.
 b('Server headless').click();assert not v('appsAllowed')
 b('continue').click();assert page.evaluate('boreaPreview.state.i')==5
 b('← back',exact=True).click();assert v('profileStage')
 b('Workstation').click();b('continue').click();count(8)
 b('Sources & install preference').click()
 # Driver sources are always locked, including after changing distributions.
 switches=v('srcSwitches');assert any(x['locked']=='true' for x in switches)
 for x in switches:
  if x['locked']=='true':
   before=x['checked'];control=page.get_by_role('switch',name=x['label'],exact=False);assert control.is_disabled();control.dispatch_event('click')
   assert next(y for y in v('srcSwitches') if y['label']==x['label'])['checked']==before
 b('Done',exact=True).click()
 expected=(v('appCount'),v('kitSize'))
 b('continue').click();b('← back',exact=True).click();assert expected==(v('appCount'),v('kitSize'))
 setstate({'i':7});rows=v('reviewRows');assert any(r['label']=='apps' and expected[0]+' apps' in r['value'] for r in rows)
 assert int(v('totalPkgs'))==sum(int(g['count'].split()[0]) for g in v('composed'))
 page.locator('.bw-hero img').wait_for();assert page.locator('.bw-hero img').evaluate('(e)=>e.complete && e.naturalWidth>0');shot('05-review.png')
 # A stranded app is surfaced and blocks continue/install, rather than disappearing.
 setstate({'i':4,'softwareStage':'apps','base':'debian','env':'workstation','noDesk':False,'appStarter':'lean','appPacks':{},'addons':{},'sel':{'brave':True},'sw':{'flathub':False,'appimage':False}})
 assert v('hasStranded') and not v('canNext')
 page.evaluate('boreaPreview.go(5)');assert page.evaluate('boreaPreview.state.i')==4
 setstate({'i':7});assert v('installBlocked');assert page.get_by_role('button',name='Install Debian',exact=True).is_disabled()
 # Matrix of supported distro/profile/hardware combinations: runtime and totals stay consistent.
 matrix=page.evaluate('''() => {const app=boreaPreview,old={...app.state},results=[];
 for(const hw of ['tuf-f15','gtx1080','rtx5070','rx7800','thinkpad'])for(const base of ['arch','debian','fedora','opensuse'])for(const env of ['minimal','server','server-gui','workstation']) {
 app.state={...old,i:4,base,env,noDesk:env==='minimal'||env==='server',softwareStage:'apps',appStarter:'everyday',appPacks:{Gaming:true,Creator:true,Developer:true},sel:{},addons:{},sw:{},pk:{}};
 const savedHw=app.props.hw;app.props.hw=hw;const x=app.renderVals();app.props.hw=savedHw;if(x.hasStranded)throw Error(base+' '+env+' stranded');if(Number(x.totalPkgs)!==x.composed.reduce((n,g)=>n+parseInt(g.count),0))throw Error('count mismatch');if(!x.appsAllowed&&x.appCount!=='0')throw Error('headless apps');results.push(base+'/'+env);}
 app.state=old;return results;}''')
 assert len(matrix)==80
 assert not errors,errors
 print('PASS: profile gating; 7-app Essentials; presets/apps/tools; removal/reset; roles; package editor; headless skip; persistence; driver source locks; Review parity; stranded-app blocking; 80 distro/environment/hardware combinations')
 browser.close()
