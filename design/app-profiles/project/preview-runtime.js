/* Local preview adapter for the supplied Design Canvas templates. No packages required. */
(() => {
  const source = window.BOREA_PREVIEW_SOURCE;
  if (!source) return;
  const doc = new DOMParser().parseFromString(source, 'text/html');
  const root = document.getElementById('preview');
  const template = doc.querySelector('x-dc');
  const script = doc.querySelector('[data-dc-script]');
  const evaluate = (expression, values) => Function('v', 'with(v){ return (' + expression + '); }')(values);
  const value = (text, values) => {
    const exact = text.match(/^\{\{(.*?)\}\}$/s);
    if (exact) return evaluate(exact[1], values);
    return text.replace(/\{\{(.*?)\}\}/gs, (_, exp) => String(evaluate(exp, values) ?? ''));
  };
  let instance;
  let restoring = false;
  let pending = false;
  class DCLogic {
    constructor(props) { this.props = props; }
    setState(patch) {
      if (restoring) return;
      Object.assign(this.state, typeof patch === 'function' ? patch(this.state) : patch);
      if (!pending) { pending = true; queueMicrotask(() => { pending = false; render(); }); }
    }
  }
  function build(node, values) {
    if (node.nodeType === 3) return document.createTextNode(value(node.textContent, values));
    if (node.nodeType !== 1) return document.createDocumentFragment();
    const tag = node.tagName.toLowerCase();
    const frag = document.createDocumentFragment();
    if (tag === 'sc-if') {
      if (value(node.getAttribute('value'), values)) for (const ch of node.childNodes) frag.append(build(ch, values));
      return frag;
    }
    if (tag === 'sc-for') {
      for (const item of value(node.getAttribute('list'), values) || []) {
        const local = Object.assign({}, values, { [node.getAttribute('as')]: item });
        for (const ch of node.childNodes) frag.append(build(ch, local));
      }
      return frag;
    }
    if (tag === 'helmet') {
      for (const ch of node.childNodes) frag.append(build(ch, values));
      return frag;
    }
    const el = node.cloneNode(false);
    for (const a of [...node.attributes]) {
      if (a.name.startsWith('hint-')) { el.removeAttribute(a.name); continue; }
      const val = value(a.value, values);
      if (/^on/i.test(a.name)) {
        el.removeAttribute(a.name);
        if (typeof val === 'function') el.addEventListener(a.name.slice(2).toLowerCase() === 'change' ? 'input' : a.name.slice(2).toLowerCase(), val);
      } else if (['disabled','checked'].includes(a.name)) {
        el.toggleAttribute(a.name, val === true || val === 'true');
      } else el.setAttribute(a.name, val ?? '');
    }
    for (const ch of node.childNodes) el.append(build(ch, values));
    return el;
  }
  function render() {
    const active = document.activeElement;
    const inputs = [...root.querySelectorAll('input')];
    const index = inputs.indexOf(active), id = active?.id;
    const selection = index >= 0 ? [active.selectionStart, active.selectionEnd] : null;
    const buttonLabel = active?.tagName === 'BUTTON' ? (active.getAttribute('aria-label') || active.textContent.trim()) : null;
    const scrolls = [...root.querySelectorAll('.apps-page,.apps-list,.apps-editor nav')].map(el => [el.className, el.scrollTop]);
    const values = instance.renderVals();
    root.replaceChildren(...[...template.childNodes].map(n => build(n, values)));
    for (const [cls, top] of scrolls) for (const el of root.getElementsByClassName(cls)) el.scrollTop = top;
    const target = id ? document.getElementById(id) : index >= 0 ? root.querySelectorAll('input')[index] : buttonLabel ? [...root.querySelectorAll('button')].find(b => (b.getAttribute('aria-label') || b.textContent.trim()) === buttonLabel) : null;
    restoring = true;
    target?.focus({preventScroll:true});
    if (selection && target && ['text','search','password'].includes(target.type)) target.setSelectionRange(...selection);
    const dialog = root.querySelector('[role=dialog]');
    if (dialog && !dialog.contains(document.activeElement)) dialog.querySelector('input,button')?.focus();
    restoring = false;
    window.boreaPreview = instance;
  }
  const Component = Function('DCLogic', script.textContent + '\nreturn Component;')(DCLogic);
  const start = new URLSearchParams(location.search).get('start') || document.body.dataset.start || 'packs';
  instance = new Component({start, motion:false});
  render();
  instance.componentDidMount?.();
})();
