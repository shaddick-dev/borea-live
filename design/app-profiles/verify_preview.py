"""Interaction checks for the supplied design prototype; requires Python Playwright + Chromium."""
from pathlib import Path
from playwright.sync_api import sync_playwright
BASE=Path(__file__).parent
with sync_playwright() as p:
    browser=p.chromium.launch(executable_path='/usr/bin/chromium',args=['--no-sandbox'])
    page=browser.new_page(viewport={'width':1280,'height':800})
    errors=[]
    page.on('pageerror', lambda error:errors.append(str(error)))
    page.goto('http://127.0.0.1:8765/index.html')
    def button(name, **kwargs): return page.get_by_role('button',name=name,**kwargs)
    def count(n): assert page.locator('.apps-totals strong').inner_text()==f'{n} apps'
    def state(): return page.evaluate('boreaPreview.state')
    def screenshot(name): page.screenshot(path=str(BASE/'previews'/name))
    assert button('continue').count()==0
    assert page.locator('[aria-pressed=true]').count()==0
    screenshot('05-start.png')
    button('Just the desktop').click();count(0)
    button('Gaming Ready').click();count(3)
    assert page.get_by_text('1 additional source needed',exact=False).count()==1
    button('Review additional sources').click()
    assert page.get_by_role('dialog').get_by_text('multilib',exact=True).count()==1
    page.keyboard.press('Escape');assert page.get_by_role('dialog').count()==0
    button('Customize apps').click()
    search=page.get_by_role('searchbox');search.fill('Steam')
    button('Steam game store').click();count(2)
    assert state()['appOverrides']['Steam']==False
    search.fill('app-that-isnt-here');assert page.get_by_text('No apps match.',exact=False).count()==1
    button('Restore profile apps').click();count(3)
    search.fill('');screenshot('05-customize.png')
    page.keyboard.press('Escape')
    assert page.get_by_role('dialog').count()==0
    button('Gaming Ready').click();count(0)
    button('Everyday Browse').click();count(6)
    button('Creative Edit').click();count(8)
    button('Development A place').click();count(11)
    screenshot('05-profiles.png')
    button('Customize apps').click();search=page.get_by_role('searchbox');search.fill('LibreOffice')
    button('LibreOffice office suite').click();count(10)
    button('Done',exact=True).click()
    assert page.get_by_text('Your own mix',exact=True).count()==2
    button('continue').click()
    button('← back',exact=True).click();count(10)
    # Review must use the same app list and estimate as the picker.
    size=page.locator('.apps-totals > span').inner_text().replace(' estimated','')
    page.evaluate("boreaPreview.go(7)")
    assert page.get_by_role('button',name=f'apps 10 apps · {size}',exact=False).count()==1
    # Source requirements follow the selected distro, and disappear with their apps.
    page.evaluate("boreaPreview.setState({i:4,base:'fedora',appStarter:'lean',specs:{gaming:true},appOverrides:{}})")
    count(3);button('Review additional sources').click()
    assert page.get_by_role('dialog').get_by_text('RPM Fusion nonfree',exact=False).count()>=1
    button('Done',exact=True).click();button('Gaming Ready').click();count(0)
    assert button('Review additional sources').count()==0
    # No horizontal overlap between profile region, summary and footer.
    for selector in ['.apps-page','.apps-dialog']:
        if page.locator(selector).count():
            assert page.locator(selector).evaluate('(e)=>e.scrollWidth<=e.clientWidth+1')
    assert not errors, errors
    print('PASS: unselected entry, lean/Everyday, pack union, app overrides, search, reset, Escape, persistence, review, distro-specific source removal, layout, local HTTP preview')
    browser.close()
