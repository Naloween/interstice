// The web platform, in JS, over the `__host` primitives (see mod.rs).
// Evaluated once per realm before any page script.
(function () {
'use strict';
const H = __host;
const G = globalThis;
const ID = Symbol('node');
const LIS = Symbol('listeners');
const PROPS = Symbol('handlers');
const STOP = Symbol('stop');

function def(obj, props) {
  for (const k of Reflect.ownKeys(props)) {
    Object.defineProperty(obj, k, { value: props[k], writable: true, configurable: true, enumerable: false });
  }
}
function getter(obj, name, get, set) {
  Object.defineProperty(obj, name, { get, set: set || undefined, configurable: true, enumerable: true });
}
function str(v) { return v === undefined || v === null ? '' : String(v); }
function domErr(name, msg) { const e = new Error(msg || name); e.name = name; return e; }

// ── console ─────────────────────────────────────────────────────────────────
function fmt(v, depth) {
  if (typeof v === 'string') return v;
  if (v instanceof Error) return v.name + ': ' + v.message + (v.stack ? '\n' + v.stack : '');
  if (v && v[ID] !== undefined) return '<' + (v.nodeName || 'node').toLowerCase() + '>';
  if (typeof v === 'function') return '[function ' + (v.name || '') + ']';
  if (v && typeof v === 'object' && (depth || 0) < 2) {
    try {
      if (Array.isArray(v)) return '[' + v.map(x => fmt(x, (depth || 0) + 1)).join(', ') + ']';
      return '{' + Object.keys(v).slice(0, 20).map(k => k + ': ' + fmt(v[k], (depth || 0) + 1)).join(', ') + '}';
    } catch (e) { return '[object]'; }
  }
  return String(v);
}
function logger(level) { return (...a) => H.log(level, a.map(x => fmt(x)).join(' ')); }
const counts = {}, timers0 = {};
G.console = {
  log: logger('log'), info: logger('info'), warn: logger('warn'), error: logger('error'),
  debug: logger('debug'), trace: logger('trace'), dir: logger('dir'), table: logger('table'),
  group: logger('group'), groupCollapsed: logger('group'), groupEnd() {},
  assert(c, ...a) { if (!c) H.log('assert', a.map(x => fmt(x)).join(' ')); },
  count(l) { l = l || 'default'; counts[l] = (counts[l] || 0) + 1; H.log('count', l + ': ' + counts[l]); },
  countReset(l) { counts[l || 'default'] = 0; },
  time(l) { timers0[l || 'default'] = H.now(); },
  timeEnd(l) { l = l || 'default'; H.log('time', l + ': ' + (H.now() - (timers0[l] || 0)) + 'ms'); },
  timeLog(l) { l = l || 'default'; H.log('time', l + ': ' + (H.now() - (timers0[l] || 0)) + 'ms'); },
};

// Seeded per document: replays draw the same numbers.
Math.random = function random() { return H.random(); };

// ── Events ──────────────────────────────────────────────────────────────────
class Event {
  constructor(type, init) {
    init = init || {};
    this.type = String(type);
    this.bubbles = !!init.bubbles;
    this.cancelable = !!init.cancelable;
    this.composed = !!init.composed;
    this.defaultPrevented = false;
    this.target = null;
    this.currentTarget = null;
    this.eventPhase = 0;
    this.isTrusted = false;
    this.timeStamp = H.now();
    this[STOP] = 0;
    this._path = [];
  }
  preventDefault() { if (this.cancelable) this.defaultPrevented = true; }
  stopPropagation() { this[STOP] |= 1; }
  stopImmediatePropagation() { this[STOP] |= 3; }
  composedPath() { return this._path.slice(); }
  get srcElement() { return this.target; }
  get cancelBubble() { return !!(this[STOP] & 1); }
  set cancelBubble(v) { if (v) this[STOP] |= 1; }
  get returnValue() { return !this.defaultPrevented; }
  set returnValue(v) { if (!v) this.preventDefault(); }
  initEvent(type, bubbles, cancelable) { this.type = String(type); this.bubbles = !!bubbles; this.cancelable = !!cancelable; }
}
Event.NONE = 0; Event.CAPTURING_PHASE = 1; Event.AT_TARGET = 2; Event.BUBBLING_PHASE = 3;

class UIEvent extends Event {
  constructor(type, init) { super(type, init); init = init || {}; this.detail = init.detail || 0; this.view = init.view || G; }
}
class MouseEvent extends UIEvent {
  constructor(type, init) {
    super(type, init); init = init || {};
    for (const k of ['screenX', 'screenY', 'clientX', 'clientY', 'offsetX', 'offsetY', 'movementX', 'movementY', 'button', 'buttons'])
      this[k] = +init[k] || 0;
    this.pageX = init.pageX !== undefined ? init.pageX : this.clientX + G.scrollX;
    this.pageY = init.pageY !== undefined ? init.pageY : this.clientY + G.scrollY;
    this.ctrlKey = !!init.ctrlKey; this.shiftKey = !!init.shiftKey; this.altKey = !!init.altKey; this.metaKey = !!init.metaKey;
    this.relatedTarget = init.relatedTarget || null;
  }
  get x() { return this.clientX; }
  get y() { return this.clientY; }
  get which() { return this.button + 1; }
  getModifierState(k) { return { Control: this.ctrlKey, Shift: this.shiftKey, Alt: this.altKey, Meta: this.metaKey }[k] || false; }
}
class PointerEvent extends MouseEvent {
  constructor(type, init) {
    super(type, init); init = init || {};
    this.pointerId = init.pointerId || 1; this.pointerType = init.pointerType || 'mouse';
    this.isPrimary = init.isPrimary !== undefined ? !!init.isPrimary : true;
    this.width = init.width || 1; this.height = init.height || 1; this.pressure = init.pressure || 0;
    this.tiltX = 0; this.tiltY = 0; this.twist = 0;
  }
  getCoalescedEvents() { return [this]; }
  getPredictedEvents() { return []; }
}
class WheelEvent extends MouseEvent {
  constructor(type, init) { super(type, init); init = init || {}; this.deltaX = +init.deltaX || 0; this.deltaY = +init.deltaY || 0; this.deltaZ = +init.deltaZ || 0; this.deltaMode = init.deltaMode || 0; }
}
WheelEvent.DOM_DELTA_PIXEL = 0; WheelEvent.DOM_DELTA_LINE = 1; WheelEvent.DOM_DELTA_PAGE = 2;
const KEYCODES = { Backspace: 8, Tab: 9, Enter: 13, Shift: 16, Control: 17, Alt: 18, Escape: 27, ' ': 32, PageUp: 33, PageDown: 34, End: 35, Home: 36, ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40, Delete: 46 };
class KeyboardEvent extends UIEvent {
  constructor(type, init) {
    super(type, init); init = init || {};
    this.key = init.key || ''; this.code = init.code || ''; this.location = init.location || 0;
    this.repeat = !!init.repeat; this.isComposing = !!init.isComposing;
    this.ctrlKey = !!init.ctrlKey; this.shiftKey = !!init.shiftKey; this.altKey = !!init.altKey; this.metaKey = !!init.metaKey;
    const kc = KEYCODES[this.key] || (this.key.length === 1 ? this.key.toUpperCase().charCodeAt(0) : 0);
    this.keyCode = init.keyCode || kc; this.which = this.keyCode;
    this.charCode = type === 'keypress' && this.key.length === 1 ? this.key.charCodeAt(0) : 0;
  }
  getModifierState(k) { return { Control: this.ctrlKey, Shift: this.shiftKey, Alt: this.altKey, Meta: this.metaKey }[k] || false; }
}
class FocusEvent extends UIEvent { constructor(type, init) { super(type, init); this.relatedTarget = (init && init.relatedTarget) || null; } }
class InputEvent extends UIEvent { constructor(type, init) { super(type, init); init = init || {}; this.data = init.data === undefined ? null : init.data; this.inputType = init.inputType || 'insertText'; this.isComposing = false; } }
class CustomEvent extends Event {
  constructor(type, init) { super(type, init); this.detail = init && init.detail !== undefined ? init.detail : null; }
  initCustomEvent(type, b, c, detail) { this.initEvent(type, b, c); this.detail = detail; }
}
class ErrorEvent extends Event { constructor(type, init) { super(type, init); init = init || {}; this.message = init.message || ''; this.error = init.error || null; this.filename = init.filename || ''; this.lineno = 0; this.colno = 0; } }
class ProgressEvent extends Event { constructor(type, init) { super(type, init); init = init || {}; this.loaded = init.loaded || 0; this.total = init.total || 0; this.lengthComputable = !!init.lengthComputable; } }
class MessageEvent extends Event { constructor(type, init) { super(type, init); init = init || {}; this.data = init.data; this.origin = init.origin || ''; this.source = init.source || null; this.ports = []; } }
class PopStateEvent extends Event { constructor(type, init) { super(type, init); this.state = init && init.state !== undefined ? init.state : null; } }
class HashChangeEvent extends Event { constructor(type, init) { super(type, init); init = init || {}; this.oldURL = init.oldURL || ''; this.newURL = init.newURL || ''; } }
class StorageEvent extends Event { constructor(type, init) { super(type, init); init = init || {}; this.key = init.key || null; this.oldValue = init.oldValue || null; this.newValue = init.newValue || null; } }
class TouchEvent extends UIEvent { constructor(type, init) { super(type, init); this.touches = []; this.targetTouches = []; this.changedTouches = []; } }
class AnimationEvent extends Event {}
class TransitionEvent extends Event {}
class SubmitEvent extends Event { constructor(type, init) { super(type, init); this.submitter = (init && init.submitter) || null; } }

const EVENT_CLASS = {
  click: MouseEvent, dblclick: MouseEvent, contextmenu: MouseEvent, auxclick: MouseEvent,
  mousedown: MouseEvent, mouseup: MouseEvent, mousemove: MouseEvent, mouseover: MouseEvent, mouseout: MouseEvent,
  mouseenter: MouseEvent, mouseleave: MouseEvent,
  pointerdown: PointerEvent, pointerup: PointerEvent, pointermove: PointerEvent, pointerover: PointerEvent,
  pointerout: PointerEvent, pointerenter: PointerEvent, pointerleave: PointerEvent, pointercancel: PointerEvent,
  wheel: WheelEvent, keydown: KeyboardEvent, keyup: KeyboardEvent, keypress: KeyboardEvent,
  focus: FocusEvent, blur: FocusEvent, focusin: FocusEvent, focusout: FocusEvent,
  input: InputEvent, beforeinput: InputEvent,
};

function invoke(node, ev, l) {
  try {
    if (typeof l === 'function') l.call(node, ev);
    else if (l && typeof l.handleEvent === 'function') l.handleEvent(ev);
  } catch (e) {
    reportError(e);
  }
}
function reportError(e) {
  console.error(e);
}

// Inline `on<type>` attribute handlers, compiled once per source text.
const compiled = new Map();
function inlineHandler(node, type) {
  const props = node[PROPS];
  if (props && Object.prototype.hasOwnProperty.call(props, type)) return props[type];
  if (node[ID] === undefined || H.kind(node[ID]) !== 1) return null;
  const src = H.getAttr(node[ID], 'on' + type);
  if (src === null) return null;
  let f = compiled.get(src);
  if (!f) {
    try { f = new Function('event', src); } catch (e) { reportError(e); f = () => {}; }
    compiled.set(src, f);
  }
  return f;
}

function fireAt(node, ev, phase) {
  const ls = node[LIS] && node[LIS][ev.type];
  ev.currentTarget = node;
  ev.eventPhase = phase;
  if (ls) {
    for (const l of ls.slice()) {
      if (l.removed) continue;
      if (phase === 1 && !l.capture) continue;
      if (phase === 3 && l.capture) continue;
      if (l.once) node.removeEventListener(ev.type, l.cb, l.capture);
      invoke(node, ev, l.cb);
      if (ev[STOP] & 2) return;
    }
  }
  if (phase !== 1) {
    const h = inlineHandler(node, ev.type);
    if (h) {
      let r;
      try { r = h.call(node, ev); } catch (e) { reportError(e); }
      if (r === false) ev.preventDefault();
    }
  }
}

// Dispatch along `path` (target first, then ancestors).
function dispatchPath(ev, path) {
  ev.target = path[0];
  ev._path = path;
  for (let i = path.length - 1; i > 0; i--) {
    fireAt(path[i], ev, 1);
    if (ev[STOP] & 1) break;
  }
  if (!(ev[STOP] & 1)) fireAt(path[0], ev, 2);
  if (ev.bubbles) {
    for (let i = 1; i < path.length && !(ev[STOP] & 1); i++) fireAt(path[i], ev, 3);
  }
  ev.currentTarget = null;
  ev.eventPhase = 0;
  return !ev.defaultPrevented;
}
function pathOf(t) {
  const path = [t];
  if (t === G) return path;
  let n = t.parentNode;
  while (n) { path.push(n); n = n.parentNode; }
  if (path[path.length - 1] === document) path.push(G);
  return path;
}

class EventTarget {
  addEventListener(type, cb, opts) {
    if (!cb) return;
    type = String(type);
    const capture = typeof opts === 'boolean' ? opts : !!(opts && opts.capture);
    const once = !!(opts && typeof opts === 'object' && opts.once);
    if (!Object.prototype.hasOwnProperty.call(this, LIS)) def(this, { [LIS]: {} });
    const ls = this[LIS][type] || (this[LIS][type] = []);
    if (ls.some(l => l.cb === cb && l.capture === capture)) return;
    ls.push({ cb, capture, once });
    H.listen(type);
    if (opts && typeof opts === 'object' && opts.signal) opts.signal.addEventListener('abort', () => this.removeEventListener(type, cb, capture));
  }
  removeEventListener(type, cb, opts) {
    const capture = typeof opts === 'boolean' ? opts : !!(opts && opts.capture);
    const ls = this[LIS] && this[LIS][type];
    if (!ls) return;
    const i = ls.findIndex(l => l.cb === cb && l.capture === capture);
    if (i >= 0) { ls[i].removed = true; ls.splice(i, 1); }
  }
  dispatchEvent(ev) {
    if (!(ev instanceof Event)) throw new TypeError('not an Event');
    ev.isTrusted = false;
    ev[STOP] = 0;
    return dispatchPath(ev, pathOf(this));
  }
}

// `on<event>` IDL properties.
const HANDLER_NAMES = ('abort blur cancel change click close contextmenu dblclick drag dragend dragenter dragleave dragover dragstart drop error focus focusin focusout input invalid keydown keypress keyup load loadend loadstart mousedown mouseenter mouseleave mousemove mouseout mouseover mouseup pointerdown pointerup pointermove pointerover pointerout pointerenter pointerleave pointercancel progress reset resize scroll select submit toggle wheel animationend transitionend touchstart touchend touchmove beforeunload hashchange message offline online pagehide pageshow popstate storage unload readystatechange visibilitychange timeout DOMContentLoaded').split(' ');
function addHandlerProps(proto) {
  for (const t of HANDLER_NAMES) {
    Object.defineProperty(proto, 'on' + t.toLowerCase(), {
      configurable: true, enumerable: true,
      get() { return (this[PROPS] && this[PROPS][t]) || null; },
      set(f) {
        if (!Object.prototype.hasOwnProperty.call(this, PROPS)) def(this, { [PROPS]: {} });
        this[PROPS][t] = typeof f === 'function' ? f : null;
        H.listen(t);
      },
    });
  }
}

// ── Nodes ───────────────────────────────────────────────────────────────────
const wrappers = new Map();
const fragments = new Set();

function wrap(id) {
  if (id === undefined || id === null || id < 0) return null;
  let w = wrappers.get(id);
  if (w) return w;
  const kind = H.kind(id);
  let cls;
  if (kind === 9) return document;
  if (kind === 3) cls = Text;
  else if (kind === 8) cls = Comment;
  else if (kind === 1) cls = fragments.has(id) ? DocumentFragment : elementClass(H.tag(id), H.ns(id));
  else return null;
  w = Object.create(cls.prototype);
  def(w, { [ID]: id });
  wrappers.set(id, w);
  return w;
}
function idOf(n) {
  if (n === null || n === undefined || n[ID] === undefined) throw new TypeError('parameter is not a Node');
  return n[ID];
}
function wrapAll(ids) { return ids.map(wrap).filter(Boolean); }

class NodeList extends Array {
  item(i) { return this[i] || null; }
  static of2(arr) { const l = new NodeList(); for (const x of arr) l.push(x); return l; }
}
Object.defineProperty(NodeList, Symbol.species, { get() { return Array; } });
const HTMLCollection = NodeList;

class Node extends EventTarget {
  get nodeType() { return H.kind(this[ID]); }
  get nodeName() { return this.nodeType === 3 ? '#text' : this.nodeType === 8 ? '#comment' : ''; }
  get nodeValue() { return this.nodeType === 1 ? null : H.text(this[ID]); }
  set nodeValue(v) { if (this.nodeType !== 1) H.setText(this[ID], str(v)); }
  get ownerDocument() { return document; }
  get parentNode() { const p = H.parent(this[ID]); return p < 0 ? null : wrap(p); }
  get parentElement() { const p = this.parentNode; return p && p.nodeType === 1 ? p : null; }
  get childNodes() { return NodeList.of2(wrapAll(H.children(this[ID]))); }
  get firstChild() { const c = H.children(this[ID]); return c.length ? wrap(c[0]) : null; }
  get lastChild() { const c = H.children(this[ID]); return c.length ? wrap(c[c.length - 1]) : null; }
  sibling(d) {
    const p = H.parent(this[ID]);
    if (p < 0) return null;
    const c = H.children(p);
    const i = c.indexOf(this[ID]);
    return i < 0 ? null : wrap(c[i + d]);
  }
  get nextSibling() { return this.sibling(1); }
  get previousSibling() { return this.sibling(-1); }
  get textContent() { return H.text(this[ID]); }
  set textContent(v) { H.setText(this[ID], str(v)); }
  get isConnected() { let n = this[ID]; while (n >= 0) { if (n === 0) return true; n = H.parent(n); } return false; }
  get baseURI() { return document.baseURI; }
  hasChildNodes() { return H.children(this[ID]).length > 0; }
  getRootNode() { let n = this; while (n.parentNode) n = n.parentNode; return n; }
  contains(o) { if (!o) return false; let n = o[ID]; while (n !== undefined && n >= 0) { if (n === this[ID]) return true; n = H.parent(n); } return false; }
  isSameNode(o) { return o === this; }
  isEqualNode(o) { return !!o && o.nodeType === this.nodeType && (this.nodeType === 1 ? o.outerHTML === this.outerHTML : o.textContent === this.textContent); }
  compareDocumentPosition(o) {
    if (o === this) return 0;
    if (this.contains(o)) return 20;
    if (o.contains(this)) return 10;
    const all = H.queryAll(0, '*');
    const a = all.indexOf(this[ID]), b = all.indexOf(o[ID]);
    if (a < 0 || b < 0) return 1;
    return a < b ? 4 : 2;
  }
  normalize() {}
  cloneNode(deep) { return wrap(H.clone(this[ID], !!deep)); }
  appendChild(c) { insertNodes(this, [c], null); return c; }
  insertBefore(c, ref) { insertNodes(this, [c], ref || null); return c; }
  removeChild(c) {
    if (!c || c.parentNode !== this) throw domErr('NotFoundError', 'not a child');
    H.remove(c[ID]);
    return c;
  }
  replaceChild(n, old) { this.insertBefore(n, old); if (n !== old) this.removeChild(old); return old; }
  lookupNamespaceURI() { return null; }
}
Object.assign(Node, { ELEMENT_NODE: 1, ATTRIBUTE_NODE: 2, TEXT_NODE: 3, CDATA_SECTION_NODE: 4, PROCESSING_INSTRUCTION_NODE: 7, COMMENT_NODE: 8, DOCUMENT_NODE: 9, DOCUMENT_TYPE_NODE: 10, DOCUMENT_FRAGMENT_NODE: 11, DOCUMENT_POSITION_DISCONNECTED: 1, DOCUMENT_POSITION_PRECEDING: 2, DOCUMENT_POSITION_FOLLOWING: 4, DOCUMENT_POSITION_CONTAINS: 8, DOCUMENT_POSITION_CONTAINED_BY: 16 });

function toNode(x) { return x instanceof Node ? x : document.createTextNode(str(x)); }

// Insert nodes (fragments expand to their children) and run any new scripts.
function insertNodes(parent, nodes, ref) {
  const pid = parent === document ? 0 : idOf(parent);
  const refId = ref ? idOf(ref) : -1;
  const added = [];
  for (const n0 of nodes) {
    const n = toNode(n0);
    const list = n instanceof DocumentFragment ? H.children(n[ID]) : [idOf(n)];
    for (const id of list) {
      if (refId >= 0) H.insertBefore(pid, id, refId); else H.append(pid, id);
      added.push(id);
    }
  }
  if (loader.started && parent.isConnected) for (const id of added) scanInserted(id);
}

class CharacterData extends Node {
  get data() { return H.text(this[ID]); }
  set data(v) { H.setText(this[ID], str(v)); }
  get length() { return this.data.length; }
  appendData(s) { this.data += s; }
  substringData(o, c) { return this.data.substr(o, c); }
  remove() { H.remove(this[ID]); }
  before(...n) { if (this.parentNode) insertNodes(this.parentNode, n, this); }
  after(...n) { if (this.parentNode) insertNodes(this.parentNode, n, this.nextSibling); }
  replaceWith(...n) { const p = this.parentNode; if (p) { insertNodes(p, n, this); this.remove(); } }
  get nextElementSibling() { let n = this.nextSibling; while (n && n.nodeType !== 1) n = n.nextSibling; return n; }
  get previousElementSibling() { let n = this.previousSibling; while (n && n.nodeType !== 1) n = n.previousSibling; return n; }
}
class Text extends CharacterData {
  get wholeText() { return this.data; }
  splitText(o) { const t = document.createTextNode(this.data.slice(o)); this.data = this.data.slice(0, o); this.after(t); return t; }
}
class Comment extends CharacterData {}

// Parent mixin (Element, Document, DocumentFragment).
const ParentMixin = {
  get children() { return NodeList.of2(wrapAll(H.children(this[ID] || 0)).filter(n => n.nodeType === 1)); },
  get childElementCount() { return this.children.length; },
  get firstElementChild() { return this.children[0] || null; },
  get lastElementChild() { const c = this.children; return c[c.length - 1] || null; },
  querySelector(sel) { return wrap(H.query(this[ID] || 0, String(sel))); },
  querySelectorAll(sel) { return NodeList.of2(wrapAll(H.queryAll(this[ID] || 0, String(sel)))); },
  getElementsByTagName(t) { return this.querySelectorAll(t === '*' ? '*' : cssEscapeIdent(t)); },
  getElementsByClassName(c) { const s = String(c).trim().split(/\s+/).filter(Boolean).map(x => '.' + cssEscapeIdent(x)).join(''); return s ? this.querySelectorAll(s) : new NodeList(); },
  getElementsByTagNameNS(ns, t) { return this.getElementsByTagName(t); },
  append(...n) { insertNodes(this, n, null); },
  prepend(...n) { insertNodes(this, n, this.firstChild); },
  replaceChildren(...n) { for (const c of this.childNodes) H.remove(c[ID]); insertNodes(this, n, null); },
};
function mixin(cls, m) {
  for (const k of Object.getOwnPropertyNames(m)) Object.defineProperty(cls.prototype, k, Object.getOwnPropertyDescriptor(m, k));
}
function cssEscapeIdent(s) { return String(s).replace(/[^a-zA-Z0-9_ -￿-]/g, c => '\\' + c); }

// ── Element ─────────────────────────────────────────────────────────────────
class DOMTokenList {
  constructor(el, attr) { def(this, { _el: el, _attr: attr }); }
  _get() { return str(this._el.getAttribute(this._attr)).split(/\s+/).filter(Boolean); }
  _set(l) { this._el.setAttribute(this._attr, l.join(' ')); }
  get length() { return this._get().length; }
  get value() { return str(this._el.getAttribute(this._attr)); }
  set value(v) { this._el.setAttribute(this._attr, v); }
  item(i) { return this._get()[i] || null; }
  contains(t) { return this._get().includes(t); }
  add(...t) { const l = this._get(); let ch = false; for (const x of t) if (!l.includes(x)) { l.push(x); ch = true; } if (ch || !this._el.hasAttribute(this._attr)) this._set(l); }
  remove(...t) { const l = this._get(); const r = l.filter(x => !t.includes(x)); if (r.length !== l.length) this._set(r); }
  toggle(t, force) {
    const has = this.contains(t);
    const want = force === undefined ? !has : !!force;
    if (want && !has) this.add(t); else if (!want && has) this.remove(t);
    return want;
  }
  replace(a, b) { const l = this._get(); const i = l.indexOf(a); if (i < 0) return false; l[i] = b; this._set(l); return true; }
  supports() { return true; }
  forEach(f, t) { this._get().forEach(f, t); }
  entries() { return this._get().entries(); }
  keys() { return this._get().keys(); }
  values() { return this._get().values(); }
  [Symbol.iterator]() { return this._get()[Symbol.iterator](); }
  toString() { return this.value; }
}

function camelToKebab(p) { return p.startsWith('--') ? p : p.replace(/[A-Z]/g, c => '-' + c.toLowerCase()).replace(/^(webkit|moz|ms)-/, '-$1-'); }
function parseStyle(s) {
  const m = new Map();
  for (const decl of str(s).split(';')) {
    const i = decl.indexOf(':');
    if (i < 0) continue;
    const k = decl.slice(0, i).trim().toLowerCase(), v = decl.slice(i + 1).trim();
    if (k) m.set(k, v);
  }
  return m;
}
function serializeStyle(m) { return Array.from(m, ([k, v]) => k + ': ' + v).join('; '); }

class CSSStyleDeclaration {
  constructor(el) { def(this, { _el: el }); }
  _map() { return parseStyle(this._el ? this._el.getAttribute('style') : ''); }
  getPropertyValue(p) { const v = this._map().get(camelToKebab(p)); return v === undefined ? '' : v.replace(/\s*!important$/, ''); }
  getPropertyPriority(p) { return /!important$/.test(this._map().get(camelToKebab(p)) || '') ? 'important' : ''; }
  setProperty(p, v, prio) {
    p = camelToKebab(p);
    const m = this._map();
    if (v === null || v === undefined || v === '') m.delete(p); else m.set(p, String(v) + (prio === 'important' ? ' !important' : ''));
    this._el.setAttribute('style', serializeStyle(m));
  }
  removeProperty(p) { const old = this.getPropertyValue(p); this.setProperty(p, ''); return old; }
  get cssText() { return str(this._el.getAttribute('style')); }
  set cssText(v) { this._el.setAttribute('style', str(v)); }
  get length() { return this._map().size; }
  item(i) { return Array.from(this._map().keys())[i] || ''; }
}
function styleProxy(el) {
  const decl = new CSSStyleDeclaration(el);
  return new Proxy(decl, {
    get(t, p) {
      if (typeof p !== 'string' || p in t) return Reflect.get(t, p);
      if (/^\d+$/.test(p)) return t.item(+p);
      return t.getPropertyValue(p);
    },
    set(t, p, v) {
      if (typeof p !== 'string' || p in t) { if (p === 'cssText') t.cssText = v; return true; }
      t.setProperty(p, v);
      return true;
    },
  });
}

class Attr {
  constructor(el, name) { this.ownerElement = el; this.name = name; this.localName = name; this.namespaceURI = null; this.prefix = null; this.specified = true; }
  get value() { return str(this.ownerElement.getAttribute(this.name)); }
  set value(v) { this.ownerElement.setAttribute(this.name, v); }
  get nodeValue() { return this.value; }
}

const SVG_NS = 'http://www.w3.org/2000/svg';
const HTML_NS = 'http://www.w3.org/1999/xhtml';

class Element extends Node {
  get nodeName() { return this.tagName; }
  get tagName() { const t = H.tag(this[ID]); return this.namespaceURI === HTML_NS ? t.toUpperCase() : t; }
  get localName() { return H.tag(this[ID]); }
  get namespaceURI() { return H.ns(this[ID]) || HTML_NS; }
  get prefix() { return null; }
  get id() { return str(H.getAttr(this[ID], 'id')); }
  set id(v) { H.setAttr(this[ID], 'id', str(v)); }
  get className() { return str(H.getAttr(this[ID], 'class')); }
  set className(v) { H.setAttr(this[ID], 'class', str(v)); }
  get classList() { return new DOMTokenList(this, 'class'); }
  get slot() { return str(this.getAttribute('slot')); }
  get attributes() {
    const list = H.attrNames(this[ID]).map(n => new Attr(this, n));
    for (const a of list) list[a.name] = a;
    list.getNamedItem = n => list.find(a => a.name === String(n).toLowerCase()) || null;
    list.item = i => list[i] || null;
    return list;
  }
  getAttribute(n) { return H.getAttr(this[ID], String(n)); }
  getAttributeNS(ns, n) { return this.getAttribute(n); }
  setAttribute(n, v) {
    n = String(n).toLowerCase();
    H.setAttr(this[ID], n, str(v));
    if (n === 'src' && this instanceof HTMLScriptElement && loader.started && this.isConnected) scanInserted(this[ID]);
  }
  setAttributeNS(ns, n, v) { this.setAttribute(n.replace(/^.*:/, ''), v); }
  removeAttribute(n) { H.removeAttr(this[ID], String(n)); }
  removeAttributeNS(ns, n) { this.removeAttribute(n); }
  hasAttribute(n) { return H.getAttr(this[ID], String(n)) !== null; }
  hasAttributeNS(ns, n) { return this.hasAttribute(n); }
  hasAttributes() { return H.attrNames(this[ID]).length > 0; }
  getAttributeNames() { return H.attrNames(this[ID]); }
  getAttributeNode(n) { return this.hasAttribute(n) ? new Attr(this, String(n).toLowerCase()) : null; }
  toggleAttribute(n, force) {
    const has = this.hasAttribute(n);
    const want = force === undefined ? !has : !!force;
    if (want && !has) this.setAttribute(n, ''); else if (!want && has) this.removeAttribute(n);
    return want;
  }
  get innerHTML() { return H.innerHTML(this[ID]); }
  set innerHTML(v) { H.setInnerHTML(this[ID], str(v)); }
  get outerHTML() { return H.outerHTML(this[ID]); }
  set outerHTML(v) {
    const p = this.parentNode;
    if (!p) return;
    const tmp = document.createElement('div');
    tmp.innerHTML = str(v);
    insertNodes(p, tmp.childNodes, this);
    this.remove();
  }
  insertAdjacentHTML(pos, html) {
    const tmp = document.createElement(this.localName === 'tbody' || this.localName === 'table' ? 'tbody' : 'div');
    H.setInnerHTML(tmp[ID], str(html));
    this.insertAdjacentNodes(pos, tmp.childNodes);
  }
  insertAdjacentNodes(pos, nodes) {
    switch (String(pos).toLowerCase()) {
      case 'beforebegin': if (this.parentNode) insertNodes(this.parentNode, nodes, this); break;
      case 'afterbegin': insertNodes(this, nodes, this.firstChild); break;
      case 'beforeend': insertNodes(this, nodes, null); break;
      case 'afterend': if (this.parentNode) insertNodes(this.parentNode, nodes, this.nextSibling); break;
      default: throw domErr('SyntaxError', 'bad position');
    }
  }
  insertAdjacentElement(pos, el) { this.insertAdjacentNodes(pos, [el]); return el; }
  insertAdjacentText(pos, t) { this.insertAdjacentNodes(pos, [document.createTextNode(str(t))]); }
  matches(sel) { return H.matches(this[ID], String(sel)); }
  webkitMatchesSelector(sel) { return this.matches(sel); }
  closest(sel) { let n = this; while (n && n.nodeType === 1) { if (n.matches(sel)) return n; n = n.parentNode; } return null; }
  remove() { H.remove(this[ID]); }
  before(...n) { if (this.parentNode) insertNodes(this.parentNode, n, this); }
  after(...n) { if (this.parentNode) insertNodes(this.parentNode, n, this.nextSibling); }
  replaceWith(...n) { const p = this.parentNode; if (p) { insertNodes(p, n, this); this.remove(); } }
  get nextElementSibling() { let n = this.nextSibling; while (n && n.nodeType !== 1) n = n.nextSibling; return n; }
  get previousElementSibling() { let n = this.previousSibling; while (n && n.nodeType !== 1) n = n.previousSibling; return n; }
  getBoundingClientRect() { const r = H.rect(this[ID]) || [0, 0, 0, 0]; return new DOMRect(r[0], r[1], r[2], r[3]); }
  getClientRects() { const r = H.rect(this[ID]); return r && (r[2] || r[3]) ? [new DOMRect(r[0], r[1], r[2], r[3])] : []; }
  _box() { return H.box(this[ID]) || [0, 0, 0, 0, 0, 0, 0, 0]; }
  get clientWidth() { return Math.round(this._box()[0]); }
  get clientHeight() { return Math.round(this._box()[1]); }
  get clientTop() { return 0; }
  get clientLeft() { return 0; }
  get scrollWidth() { return Math.round(this._box()[2]); }
  get scrollHeight() { return Math.round(this._box()[3]); }
  get scrollLeft() { return this._isRoot() ? G.scrollX : this._box()[4]; }
  set scrollLeft(v) { if (this._isRoot()) G.scrollTo(+v, G.scrollY); else H.scrollTo(this[ID], +v, NaN); }
  get scrollTop() { return this._isRoot() ? G.scrollY : this._box()[5]; }
  set scrollTop(v) { if (this._isRoot()) G.scrollTo(G.scrollX, +v); else H.scrollTo(this[ID], NaN, +v); }
  _isRoot() { return this === document.documentElement || this === document.body; }
  scrollTo(x, y) { if (x && typeof x === 'object') { y = x.top; x = x.left; } if (this._isRoot()) G.scrollTo(x, y); else H.scrollTo(this[ID], x === undefined ? NaN : +x, y === undefined ? NaN : +y); }
  scroll(x, y) { this.scrollTo(x, y); }
  scrollBy(x, y) { if (x && typeof x === 'object') { y = x.top; x = x.left; } this.scrollTo(this.scrollLeft + (+x || 0), this.scrollTop + (+y || 0)); }
  scrollIntoView(opt) {
    const block = opt === false ? 'end' : (opt && typeof opt === 'object' && opt.block) || 'start';
    H.scrollIntoView(this[ID], block);
  }
  scrollIntoViewIfNeeded() { H.scrollIntoView(this[ID], 'nearest'); }
  attachShadow() { return this; }
  get shadowRoot() { return null; }
  animate() { return { finished: Promise.resolve(), cancel() {}, play() {}, pause() {}, onfinish: null, addEventListener() {} }; }
  getAnimations() { return []; }
  setPointerCapture() {}
  releasePointerCapture() {}
  hasPointerCapture() { return false; }
  requestFullscreen() { return Promise.reject(new Error('unsupported')); }
}
mixin(Element, ParentMixin);

const camel = s => s.replace(/-([a-z])/g, (_, c) => c.toUpperCase());
function reflect(cls, props) {
  for (const spec of props) {
    const [prop, attr, type] = spec.split(':').concat(['', ''])
      .slice(0, 3).map((x, i) => (i === 1 && !x ? spec.split(':')[0].toLowerCase() : x));
    const a = attr || prop.toLowerCase();
    if (type === 'b') getter(cls.prototype, prop, function () { return this.hasAttribute(a); }, function (v) { this.toggleAttribute(a, !!v); });
    else if (type === 'n') getter(cls.prototype, prop, function () { const n = parseInt(this.getAttribute(a), 10); return isNaN(n) ? 0 : n; }, function (v) { this.setAttribute(a, String(v | 0)); });
    else if (type === 'u') getter(cls.prototype, prop, function () { const v = this.getAttribute(a); if (v === null) return ''; const u = H.urlParse(v.trim(), document.baseURI); return u ? u[0] : v; }, function (v) { this.setAttribute(a, v); });
    else getter(cls.prototype, prop, function () { return str(this.getAttribute(a)); }, function (v) { this.setAttribute(a, v); });
  }
}

class HTMLElement extends Element {
  get innerText() { return this.textContent; }
  set innerText(v) { this.textContent = v; }
  get outerText() { return this.textContent; }
  get dataset() {
    const el = this;
    return new Proxy({}, {
      get(_, p) { if (typeof p !== 'string') return undefined; const v = el.getAttribute('data-' + camelToKebab(p)); return v === null ? undefined : v; },
      set(_, p, v) { el.setAttribute('data-' + camelToKebab(p), v); return true; },
      deleteProperty(_, p) { el.removeAttribute('data-' + camelToKebab(p)); return true; },
      has(_, p) { return el.hasAttribute('data-' + camelToKebab(p)); },
      ownKeys() { return el.getAttributeNames().filter(n => n.startsWith('data-')).map(n => camel(n.slice(5))); },
      getOwnPropertyDescriptor(_, p) { const v = el.getAttribute('data-' + camelToKebab(p)); return v === null ? undefined : { value: v, enumerable: true, configurable: true, writable: true }; },
    });
  }
  get style() { return styleProxy(this); }
  set style(v) { this.setAttribute('style', v); }
  get tabIndex() { const n = parseInt(this.getAttribute('tabindex'), 10); return isNaN(n) ? (/^(a|button|input|select|textarea)$/.test(this.localName) ? 0 : -1) : n; }
  set tabIndex(v) { this.setAttribute('tabindex', String(v | 0)); }
  get offsetParent() { let p = this.parentElement; while (p && p !== document.body) { const pos = H.computed(p[ID], 'position'); if (pos && pos !== 'static') return p; p = p.parentElement; } return document.body; }
  get offsetTop() { const r = H.rect(this[ID]); const p = this.offsetParent; const pr = p && p !== document.body ? H.rect(p[ID]) : [0, -G.scrollY]; return r ? Math.round(r[1] - pr[1]) : 0; }
  get offsetLeft() { const r = H.rect(this[ID]); const p = this.offsetParent; const pr = p && p !== document.body ? H.rect(p[ID]) : [-G.scrollX, 0]; return r ? Math.round(r[0] - pr[0]) : 0; }
  get offsetWidth() { const r = H.rect(this[ID]); return r ? Math.round(r[2]) : 0; }
  get offsetHeight() { const r = H.rect(this[ID]); return r ? Math.round(r[3]) : 0; }
  get isContentEditable() { return this.getAttribute('contenteditable') === 'true'; }
  focus() {
    const prev = document.activeElement;
    if (prev === this) return;
    H.focus(this[ID]);
    if (prev && prev !== document.body) { fire(prev, 'blur', FocusEvent, { relatedTarget: this }); fire(prev, 'focusout', FocusEvent, { bubbles: true, relatedTarget: this }); }
    fire(this, 'focus', FocusEvent, { relatedTarget: prev }); fire(this, 'focusin', FocusEvent, { bubbles: true, relatedTarget: prev });
  }
  blur() {
    if (document.activeElement !== this) return;
    H.blur();
    fire(this, 'blur', FocusEvent, {}); fire(this, 'focusout', FocusEvent, { bubbles: true });
  }
  click() {
    if (this.disabled) return;
    const ev = new MouseEvent('click', { bubbles: true, cancelable: true, button: 0, detail: 1 });
    const ok = this.dispatchEvent(ev);
    if (ok) activate(this);
  }
}
reflect(HTMLElement, ['title', 'lang', 'dir', 'accessKey:accesskey', 'hidden::b', 'draggable', 'contentEditable:contenteditable', 'translate', 'inputMode:inputmode', 'enterKeyHint:enterkeyhint', 'nonce', 'autofocus::b', 'popover']);
addHandlerProps(HTMLElement.prototype);

function fire(target, type, cls, init) {
  const ev = new (cls || Event)(type, init || {});
  return target.dispatchEvent(ev);
}

// Default action of a synthetic `click()`.
function activate(el) {
  for (let n = el; n && n.nodeType === 1; n = n.parentElement) {
    if (n.localName === 'a' && n.hasAttribute('href')) { navigateTo(n.href, false); return; }
    if (n.localName === 'input' && (n.type === 'checkbox' || n.type === 'radio')) {
      n.checked = n.type === 'radio' ? true : !n.checked;
      fire(n, 'input', InputEvent, { bubbles: true }); fire(n, 'change', Event, { bubbles: true });
      return;
    }
    if ((n.localName === 'button' && n.type === 'submit') || (n.localName === 'input' && n.type === 'submit')) {
      const f = n.form; if (f) f.requestSubmit(n); return;
    }
    if (n.localName === 'summary' && n.parentElement && n.parentElement.localName === 'details') { n.parentElement.open = !n.parentElement.open; return; }
    if (n.localName === 'label') { const c = n.control; if (c && c !== el) c.click(); return; }
  }
}

class HTMLAnchorElement extends HTMLElement {
  _u() { return H.urlParse(this.href || '') || ['', '', '', '', '', '', '', '', '', '', '']; }
  get protocol() { return this._u()[1]; }
  get username() { return this._u()[2]; }
  get password() { return this._u()[3]; }
  get host() { return this._u()[4]; }
  get hostname() { return this._u()[5]; }
  get port() { return this._u()[6]; }
  get pathname() { return this._u()[7]; }
  get search() { return this._u()[8]; }
  get hash() { return this._u()[9]; }
  set hash(v) { const u = new URL(this.href); u.hash = v; this.href = u.href; }
  get origin() { return this._u()[10]; }
  get text() { return this.textContent; }
  toString() { return this.href; }
}
reflect(HTMLAnchorElement, ['href::u', 'target', 'rel', 'download', 'hreflang', 'type', 'referrerPolicy:referrerpolicy', 'ping']);
getter(HTMLAnchorElement.prototype, 'relList', function () { return new DOMTokenList(this, 'rel'); });
class HTMLAreaElement extends HTMLAnchorElement {}

function formOf(el) {
  const fid = el.getAttribute('form');
  if (fid) return document.getElementById(fid);
  return el.closest('form');
}

class HTMLInputElement extends HTMLElement {
  get type() { const t = str(this.getAttribute('type')).toLowerCase(); return t || 'text'; }
  set type(v) { this.setAttribute('type', v); }
  get value() {
    const v = H.value(this[ID]);
    if (v !== null) return v;
    const a = this.getAttribute('value');
    return a === null ? (this.type === 'checkbox' || this.type === 'radio' ? 'on' : '') : a;
  }
  set value(v) {
    if (H.value(this[ID]) !== null) H.setValue(this[ID], str(v)); else this.setAttribute('value', str(v));
  }
  get defaultValue() { return str(this.getAttribute('value')); }
  set defaultValue(v) { this.setAttribute('value', v); }
  get checked() { const c = H.checked(this[ID]); return c === null ? this.hasAttribute('checked') : c; }
  set checked(v) {
    if (H.checked(this[ID]) === null) { this.toggleAttribute('checked', !!v); return; }
    H.setChecked(this[ID], !!v);
    if (v && this.type === 'radio' && this.name) {
      for (const o of document.querySelectorAll('input[type=radio][name="' + this.name.replace(/"/g, '\\"') + '"]')) if (o !== this) H.setChecked(o[ID], false);
    }
  }
  get defaultChecked() { return this.hasAttribute('checked'); }
  get form() { return formOf(this); }
  get valueAsNumber() { return parseFloat(this.value); }
  set valueAsNumber(v) { this.value = String(v); }
  get files() { return []; }
  get labels() { return this.id ? Array.from(document.querySelectorAll('label[for="' + this.id + '"]')) : []; }
  get validity() { return { valid: this.checkValidity(), valueMissing: this.required && !this.value }; }
  get validationMessage() { return ''; }
  get willValidate() { return true; }
  get selectionStart() { return this.value.length; }
  get selectionEnd() { return this.value.length; }
  set selectionStart(v) {}
  set selectionEnd(v) {}
  select() { this.focus(); }
  setSelectionRange() {}
  setRangeText(t) { this.value = t; }
  checkValidity() { return !(this.required && !this.value); }
  reportValidity() { return this.checkValidity(); }
  setCustomValidity() {}
  stepUp(n) { this.value = String((parseFloat(this.value) || 0) + (n || 1)); }
  stepDown(n) { this.value = String((parseFloat(this.value) || 0) - (n || 1)); }
  showPicker() {}
}
reflect(HTMLInputElement, ['name', 'placeholder', 'disabled::b', 'readOnly:readonly:b', 'required::b', 'multiple::b', 'autocomplete', 'min', 'max', 'step', 'pattern', 'accept', 'alt', 'src::u', 'size::n', 'maxLength:maxlength:n', 'minLength:minlength:n', 'formAction:formaction', 'indeterminate::b']);
class HTMLTextAreaElement extends HTMLElement {
  get value() { const v = H.value(this[ID]); return v !== null ? v : this.textContent; }
  set value(v) { if (H.value(this[ID]) !== null) H.setValue(this[ID], str(v)); else this.textContent = v; }
  get defaultValue() { return this.textContent; }
  set defaultValue(v) { this.textContent = v; }
  get type() { return 'textarea'; }
  get form() { return formOf(this); }
  get textLength() { return this.value.length; }
  get selectionStart() { return this.value.length; }
  get selectionEnd() { return this.value.length; }
  set selectionStart(v) {}
  set selectionEnd(v) {}
  select() { this.focus(); }
  setSelectionRange() {}
  checkValidity() { return !(this.required && !this.value); }
  reportValidity() { return this.checkValidity(); }
  setCustomValidity() {}
}
reflect(HTMLTextAreaElement, ['name', 'placeholder', 'disabled::b', 'readOnly:readonly:b', 'required::b', 'rows::n', 'cols::n', 'wrap', 'autocomplete', 'maxLength:maxlength:n']);
class HTMLButtonElement extends HTMLElement {
  get type() { const t = str(this.getAttribute('type')).toLowerCase(); return t === 'button' || t === 'reset' ? t : 'submit'; }
  set type(v) { this.setAttribute('type', v); }
  get form() { return formOf(this); }
  checkValidity() { return true; }
}
reflect(HTMLButtonElement, ['name', 'value', 'disabled::b', 'formAction:formaction']);

class HTMLOptionElement extends HTMLElement {
  get value() { const v = this.getAttribute('value'); return v === null ? this.textContent.trim() : v; }
  set value(v) { this.setAttribute('value', v); }
  get text() { return this.textContent; }
  set text(v) { this.textContent = v; }
  get selected() { return this.hasAttribute('selected'); }
  set selected(v) {
    const sel = this.closest('select');
    if (v && sel && !sel.multiple) for (const o of sel.options) if (o !== this) o.removeAttribute('selected');
    this.toggleAttribute('selected', !!v);
  }
  get defaultSelected() { return this.hasAttribute('selected'); }
  get index() { const s = this.closest('select'); return s ? s.options.indexOf(this) : 0; }
  get form() { const s = this.closest('select'); return s ? s.form : null; }
}
reflect(HTMLOptionElement, ['label', 'disabled::b']);
class HTMLSelectElement extends HTMLElement {
  get options() { const l = NodeList.of2(Array.from(this.querySelectorAll('option'))); l.add = (o, b) => this.add(o, b); l.remove = i => this.remove(i); getter(l, 'selectedIndex', () => this.selectedIndex, v => { this.selectedIndex = v; }); return l; }
  get selectedOptions() { return this.options.filter(o => o.selected); }
  get selectedIndex() { const o = this.options; const i = o.findIndex(x => x.selected); return i >= 0 ? i : (o.length && !this.multiple ? 0 : -1); }
  set selectedIndex(i) { const o = this.options; o.forEach((x, j) => x.toggleAttribute('selected', j === i)); }
  get value() { const o = this.options[this.selectedIndex]; return o ? o.value : ''; }
  set value(v) { const o = this.options; const i = o.findIndex(x => x.value === String(v)); this.selectedIndex = i; }
  get length() { return this.options.length; }
  get type() { return this.multiple ? 'select-multiple' : 'select-one'; }
  get form() { return formOf(this); }
  item(i) { return this.options[i] || null; }
  add(o, before) { this.insertBefore(o, typeof before === 'number' ? this.options[before] || null : before || null); }
  remove(i) { if (i === undefined) { Element.prototype.remove.call(this); return; } const o = this.options[i]; if (o) o.remove(); }
  checkValidity() { return true; }
}
reflect(HTMLSelectElement, ['name', 'disabled::b', 'multiple::b', 'required::b', 'size::n']);

class HTMLFormElement extends HTMLElement {
  get elements() { const l = NodeList.of2(Array.from(this.querySelectorAll('input,select,textarea,button,fieldset,output'))); l.namedItem = n => l.find(e => e.name === n || e.id === n) || null; for (const e of l) if (e.name && !(e.name in l)) l[e.name] = e; return l; }
  get length() { return this.elements.length; }
  submit() { this._submit(null); }
  requestSubmit(submitter) {
    if (!this.checkValidity()) return;
    const ev = new SubmitEvent('submit', { bubbles: true, cancelable: true, submitter: submitter || null });
    if (this.dispatchEvent(ev)) this._submit(submitter || null);
  }
  _submit(submitter) {
    const method = (this.getAttribute('method') || 'get').toLowerCase();
    const action = (submitter && submitter.getAttribute('formaction')) || this.action;
    const fd = new FormData(this);
    if (submitter && submitter.name) fd.append(submitter.name, submitter.value);
    if (method !== 'get') console.warn('form POST is not supported; submitting as GET');
    const u = new URL(action);
    u.search = new URLSearchParams(fd).toString();
    navigateTo(u.href, false);
  }
  reset() { for (const e of this.elements) { if ('defaultValue' in e && e.localName !== 'select') e.value = e.defaultValue; if (e.localName === 'input' && (e.type === 'checkbox' || e.type === 'radio')) e.checked = e.defaultChecked; } fire(this, 'reset', Event, { bubbles: true }); }
  checkValidity() { return this.elements.every(e => !e.checkValidity || e.checkValidity()); }
  reportValidity() { return this.checkValidity(); }
}
getter(HTMLFormElement.prototype, 'action', function () { const a = this.getAttribute('action'); const u = H.urlParse(a ? a.trim() : '', document.baseURI); return u ? u[0] : document.URL; }, function (v) { this.setAttribute('action', v); });
reflect(HTMLFormElement, ['method', 'name', 'target', 'enctype', 'noValidate:novalidate:b', 'acceptCharset:accept-charset', 'autocomplete']);
class HTMLLabelElement extends HTMLElement {
  get control() { const f = this.htmlFor; return f ? document.getElementById(f) : this.querySelector('input,select,textarea,button'); }
  get form() { const c = this.control; return c ? c.form : null; }
}
reflect(HTMLLabelElement, ['htmlFor:for']);
class HTMLFieldSetElement extends HTMLElement { get elements() { return this.querySelectorAll('input,select,textarea,button'); } }
reflect(HTMLFieldSetElement, ['disabled::b', 'name']);
class HTMLOutputElement extends HTMLElement { get value() { return this.textContent; } set value(v) { this.textContent = v; } }

class HTMLImageElement extends HTMLElement {
  get complete() { return true; }
  get naturalWidth() { return this.offsetWidth; }
  get naturalHeight() { return this.offsetHeight; }
  get width() { const a = parseInt(this.getAttribute('width'), 10); return isNaN(a) ? this.offsetWidth : a; }
  set width(v) { this.setAttribute('width', String(v)); }
  get height() { const a = parseInt(this.getAttribute('height'), 10); return isNaN(a) ? this.offsetHeight : a; }
  set height(v) { this.setAttribute('height', String(v)); }
  get currentSrc() { return this.src; }
  decode() { return Promise.resolve(); }
}
reflect(HTMLImageElement, ['src::u', 'alt', 'srcset', 'sizes', 'crossOrigin:crossorigin', 'useMap:usemap', 'loading', 'decoding', 'referrerPolicy:referrerpolicy', 'isMap:ismap:b', 'fetchPriority:fetchpriority']);
class HTMLScriptElement extends HTMLElement {
  get text() { return this.textContent; }
  set text(v) { this.textContent = v; }
}
reflect(HTMLScriptElement, ['src::u', 'type', 'async::b', 'defer::b', 'crossOrigin:crossorigin', 'integrity', 'noModule:nomodule:b', 'charset', 'referrerPolicy:referrerpolicy']);
class HTMLLinkElement extends HTMLElement { get sheet() { return null; } get relList() { return new DOMTokenList(this, 'rel'); } }
reflect(HTMLLinkElement, ['href::u', 'rel', 'type', 'media', 'as', 'crossOrigin:crossorigin', 'hreflang', 'sizes', 'integrity', 'disabled::b']);
class HTMLStyleElement extends HTMLElement { get sheet() { return { cssRules: [], insertRule() { return 0; }, deleteRule() {} }; } }
reflect(HTMLStyleElement, ['media', 'type', 'disabled::b']);
class HTMLMetaElement extends HTMLElement {}
reflect(HTMLMetaElement, ['name', 'content', 'httpEquiv:http-equiv', 'charset']);
class HTMLBaseElement extends HTMLElement {}
reflect(HTMLBaseElement, ['href::u', 'target']);
class HTMLTitleElement extends HTMLElement { get text() { return this.textContent; } set text(v) { this.textContent = v; } }
class HTMLIFrameElement extends HTMLElement { get contentWindow() { return null; } get contentDocument() { return null; } }
reflect(HTMLIFrameElement, ['src::u', 'name', 'width', 'height', 'allow', 'loading', 'srcdoc', 'sandbox']);
class HTMLCanvasElement extends HTMLElement {
  getContext() { return null; }
  toDataURL() { return 'data:,'; }
  toBlob(cb) { cb(null); }
}
reflect(HTMLCanvasElement, ['width::n', 'height::n']);
class HTMLMediaElement extends HTMLElement {
  play() { return Promise.reject(domErr('NotSupportedError', 'media playback is not supported')); }
  pause() {}
  load() {}
  canPlayType() { return ''; }
  get paused() { return true; }
  get currentTime() { return 0; }
  set currentTime(v) {}
  get duration() { return NaN; }
  get readyState() { return 0; }
  get volume() { return 1; }
  set volume(v) {}
  get muted() { return this.hasAttribute('muted'); }
  set muted(v) { this.toggleAttribute('muted', !!v); }
}
reflect(HTMLMediaElement, ['src::u', 'autoplay::b', 'loop::b', 'controls::b', 'preload', 'poster::u']);
class HTMLVideoElement extends HTMLMediaElement {}
class HTMLAudioElement extends HTMLMediaElement {}
class HTMLTemplateElement extends HTMLElement {
  get content() {
    const f = document.createDocumentFragment();
    H.setInnerHTML(f[ID], this.innerHTML);
    return f;
  }
}
class HTMLDetailsElement extends HTMLElement {}
reflect(HTMLDetailsElement, ['open::b', 'name']);
class HTMLDialogElement extends HTMLElement {
  show() { this.open = true; }
  showModal() { this.open = true; }
  close(rv) { if (rv !== undefined) this.returnValue = rv; this.open = false; fire(this, 'close'); }
}
reflect(HTMLDialogElement, ['open::b']);
class HTMLTableElement extends HTMLElement {
  get rows() { return this.querySelectorAll('tr'); }
  get tBodies() { return this.querySelectorAll('tbody'); }
  get tHead() { return this.querySelector('thead'); }
  get tFoot() { return this.querySelector('tfoot'); }
  get caption() { return this.querySelector('caption'); }
  insertRow(i) { const tr = document.createElement('tr'); const b = this.tBodies[0] || this; const r = b.querySelectorAll('tr'); b.insertBefore(tr, i === undefined || i < 0 ? null : r[i] || null); return tr; }
  deleteRow(i) { const r = this.rows[i]; if (r) r.remove(); }
}
class HTMLTableRowElement extends HTMLElement {
  get cells() { return this.children.filter(c => c.localName === 'td' || c.localName === 'th'); }
  get rowIndex() { const t = this.closest('table'); return t ? t.rows.indexOf(this) : -1; }
  insertCell(i) { const td = document.createElement('td'); const c = this.cells; this.insertBefore(td, i === undefined || i < 0 ? null : c[i] || null); return td; }
  deleteCell(i) { const c = this.cells[i]; if (c) c.remove(); }
}
class HTMLTableCellElement extends HTMLElement { get cellIndex() { const r = this.parentElement; return r ? r.cells.indexOf(this) : -1; } }
reflect(HTMLTableCellElement, ['colSpan:colspan:n', 'rowSpan:rowspan:n', 'headers', 'scope']);
class HTMLTableSectionElement extends HTMLElement {
  get rows() { return this.children.filter(c => c.localName === 'tr'); }
  insertRow(i) { const tr = document.createElement('tr'); const r = this.rows; this.insertBefore(tr, i === undefined || i < 0 ? null : r[i] || null); return tr; }
  deleteRow(i) { const r = this.rows[i]; if (r) r.remove(); }
}
class HTMLLIElement extends HTMLElement {}
reflect(HTMLLIElement, ['value::n']);
class HTMLOListElement extends HTMLElement {}
reflect(HTMLOListElement, ['start::n', 'reversed::b', 'type']);
class HTMLProgressElement extends HTMLElement {}
reflect(HTMLProgressElement, ['value', 'max']);
class HTMLDivElement extends HTMLElement {}
class HTMLSpanElement extends HTMLElement {}
class HTMLParagraphElement extends HTMLElement {}
class HTMLHeadingElement extends HTMLElement {}
class HTMLUListElement extends HTMLElement {}
class HTMLBodyElement extends HTMLElement {}
class HTMLHeadElement extends HTMLElement {}
class HTMLHtmlElement extends HTMLElement {}
class HTMLPreElement extends HTMLElement {}
class HTMLBRElement extends HTMLElement {}
class HTMLHRElement extends HTMLElement {}
class HTMLSourceElement extends HTMLElement {}
reflect(HTMLSourceElement, ['src::u', 'srcset', 'type', 'media', 'sizes']);
class HTMLPictureElement extends HTMLElement {}
class HTMLUnknownElement extends HTMLElement {}
class HTMLSlotElement extends HTMLElement { assignedNodes() { return []; } assignedElements() { return []; } }
class SVGElement extends Element {
  get style() { return styleProxy(this); }
  get dataset() { return HTMLElement.prototype.__lookupGetter__('dataset').call(this); }
  get className() { return { baseVal: str(this.getAttribute('class')), animVal: str(this.getAttribute('class')) }; }
  set className(v) { this.setAttribute('class', v); }
  getBBox() { const r = H.rect(this[ID]) || [0, 0, 0, 0]; return new DOMRect(r[0], r[1], r[2], r[3]); }
  focus() { HTMLElement.prototype.focus.call(this); }
  blur() { HTMLElement.prototype.blur.call(this); }
}
addHandlerProps(SVGElement.prototype);
class SVGSVGElement extends SVGElement { createSVGPoint() { return { x: 0, y: 0, matrixTransform() { return this; } }; } }

const TAGS = {
  a: HTMLAnchorElement, area: HTMLAreaElement, input: HTMLInputElement, textarea: HTMLTextAreaElement, button: HTMLButtonElement,
  select: HTMLSelectElement, option: HTMLOptionElement, form: HTMLFormElement, label: HTMLLabelElement, fieldset: HTMLFieldSetElement,
  output: HTMLOutputElement, img: HTMLImageElement, script: HTMLScriptElement, link: HTMLLinkElement, style: HTMLStyleElement,
  meta: HTMLMetaElement, base: HTMLBaseElement, title: HTMLTitleElement, iframe: HTMLIFrameElement, canvas: HTMLCanvasElement,
  video: HTMLVideoElement, audio: HTMLAudioElement, template: HTMLTemplateElement, details: HTMLDetailsElement, dialog: HTMLDialogElement,
  table: HTMLTableElement, tr: HTMLTableRowElement, td: HTMLTableCellElement, th: HTMLTableCellElement, thead: HTMLTableSectionElement,
  tbody: HTMLTableSectionElement, tfoot: HTMLTableSectionElement, li: HTMLLIElement, ol: HTMLOListElement, ul: HTMLUListElement,
  progress: HTMLProgressElement, div: HTMLDivElement, span: HTMLSpanElement, p: HTMLParagraphElement, h1: HTMLHeadingElement,
  h2: HTMLHeadingElement, h3: HTMLHeadingElement, h4: HTMLHeadingElement, h5: HTMLHeadingElement, h6: HTMLHeadingElement,
  body: HTMLBodyElement, head: HTMLHeadElement, html: HTMLHtmlElement, pre: HTMLPreElement, br: HTMLBRElement, hr: HTMLHRElement,
  source: HTMLSourceElement, picture: HTMLPictureElement, slot: HTMLSlotElement,
};
const customElementClasses = new Map();
function elementClass(tag, ns) {
  if (ns === SVG_NS) return tag === 'svg' ? SVGSVGElement : SVGElement;
  if (ns && ns !== HTML_NS) return Element;
  return TAGS[tag] || customElementClasses.get(tag) || HTMLElement;
}

class DocumentFragment extends Node {
  get nodeType() { return 11; }
  get nodeName() { return '#document-fragment'; }
  get parentNode() { return null; }
  getElementById(id) { return this.querySelector('#' + cssEscapeIdent(id)); }
  get innerHTML() { return H.innerHTML(this[ID]); }
}
mixin(DocumentFragment, ParentMixin);

class DOMRect {
  constructor(x, y, w, h) { this.x = x || 0; this.y = y || 0; this.width = w || 0; this.height = h || 0; }
  get left() { return Math.min(this.x, this.x + this.width); }
  get top() { return Math.min(this.y, this.y + this.height); }
  get right() { return Math.max(this.x, this.x + this.width); }
  get bottom() { return Math.max(this.y, this.y + this.height); }
  toJSON() { return { x: this.x, y: this.y, width: this.width, height: this.height, top: this.top, left: this.left, right: this.right, bottom: this.bottom }; }
  static fromRect(r) { r = r || {}; return new DOMRect(r.x, r.y, r.width, r.height); }
}

// ── Document ────────────────────────────────────────────────────────────────
let cookieJar = new Map();
let currentScript = null;
let readyState = 'loading';

class Document extends Node {
  get nodeType() { return 9; }
  get nodeName() { return '#document'; }
  get parentNode() { return null; }
  get documentElement() { return wrap(H.query(0, 'html')) || this.firstElementChild; }
  get head() { return wrap(H.query(0, 'head')); }
  get body() { return wrap(H.query(0, 'body')) || wrap(H.query(0, 'frameset')); }
  get title() { const t = wrap(H.query(0, 'title')); return t ? t.textContent.replace(/\s+/g, ' ').trim() : ''; }
  set title(v) {
    let t = wrap(H.query(0, 'title'));
    if (!t && this.head) { t = this.createElement('title'); this.head.appendChild(t); }
    if (t) t.textContent = v;
  }
  get URL() { return location.href; }
  get documentURI() { return location.href; }
  get baseURI() { const b = wrap(H.query(0, 'base[href]')); return b ? b.href : H.url(); }
  get referrer() { return ''; }
  get domain() { return location.hostname; }
  get origin() { return location.origin; }
  get location() { return location; }
  set location(v) { location.href = v; }
  get defaultView() { return G; }
  get readyState() { return readyState; }
  get characterSet() { return 'UTF-8'; }
  get charset() { return 'UTF-8'; }
  get inputEncoding() { return 'UTF-8'; }
  get contentType() { return 'text/html'; }
  get compatMode() { return 'CSS1Compat'; }
  get doctype() { return { name: 'html', publicId: '', systemId: '', nodeType: 10 }; }
  get hidden() { return false; }
  get visibilityState() { return 'visible'; }
  get activeElement() { const a = H.activeElement(); return a >= 0 ? wrap(a) : this.body; }
  get currentScript() { return currentScript; }
  get scrollingElement() { return this.documentElement; }
  get forms() { return this.querySelectorAll('form'); }
  get images() { return this.querySelectorAll('img'); }
  get links() { return this.querySelectorAll('a[href],area[href]'); }
  get scripts() { return this.querySelectorAll('script'); }
  get styleSheets() { return []; }
  get fullscreenElement() { return null; }
  get pointerLockElement() { return null; }
  get designMode() { return 'off'; }
  set designMode(v) {}
  get cookie() { return Array.from(cookieJar, ([k, v]) => k + '=' + v).join('; '); }
  set cookie(v) {
    const first = String(v).split(';')[0];
    const i = first.indexOf('=');
    if (i < 0) return;
    const k = first.slice(0, i).trim(), val = first.slice(i + 1).trim();
    if (/;\s*(max-age=0|expires=thu, 01 jan 1970)/i.test(v)) cookieJar.delete(k); else cookieJar.set(k, val);
  }
  getElementById(id) { return wrap(H.byId(String(id))); }
  getElementsByName(n) { return this.querySelectorAll('[name="' + String(n).replace(/"/g, '\\"') + '"]'); }
  createElement(tag, opts) {
    const w = wrap(H.createElement(String(tag), ''));
    if (opts && opts.is) w.setAttribute('is', opts.is);
    return w;
  }
  createElementNS(ns, tag) { return wrap(H.createElement(String(tag).replace(/^.*:/, ''), ns || '')); }
  createTextNode(t) { return wrap(H.createText(str(t))); }
  createComment(t) { return wrap(H.createComment(str(t))); }
  createDocumentFragment() { const id = H.createElement('template', ''); fragments.add(id); return wrap(id); }
  createEvent(kind) { const c = { mouseevent: MouseEvent, mouseevents: MouseEvent, keyboardevent: KeyboardEvent, customevent: CustomEvent, uievent: UIEvent, uievents: UIEvent, focusevent: FocusEvent }[String(kind).toLowerCase()] || Event; return new c(''); }
  createRange() { return new Range(); }
  createTreeWalker(root, what, filter) { return new TreeWalker(root, what, filter); }
  createNodeIterator(root, what, filter) { return new TreeWalker(root, what, filter); }
  createAttribute(n) { return { name: n, value: '' }; }
  importNode(n, deep) { return n.cloneNode(deep); }
  adoptNode(n) { return n; }
  hasFocus() { return true; }
  getSelection() { return G.getSelection(); }
  elementFromPoint(x, y) { return wrap(H.hit(+x, +y)); }
  elementsFromPoint(x, y) { const e = this.elementFromPoint(x, y); const r = []; for (let n = e; n && n.nodeType === 1; n = n.parentElement) r.push(n); return r; }
  caretPositionFromPoint() { return null; }
  execCommand() { return false; }
  queryCommandSupported() { return false; }
  exitFullscreen() { return Promise.resolve(); }
  exitPointerLock() {}
  open() { return this; }
  close() {}
  write(...parts) {
    const html = parts.join('');
    const anchor = currentScript;
    const tmp = this.createElement('div');
    H.setInnerHTML(tmp[ID], html);
    const nodes = tmp.childNodes;
    if (anchor && anchor.parentNode) insertNodes(anchor.parentNode, nodes, anchor.nextSibling);
    else if (this.body) insertNodes(this.body, nodes, null);
  }
  writeln(...parts) { this.write(...parts, '\n'); }
  get implementation() {
    return {
      createHTMLDocument: () => new DetachedDocument(),
      hasFeature: () => true,
    };
  }
  get fonts() { return fontSet; }
  get timeline() { return { currentTime: H.now() }; }
}
mixin(Document, ParentMixin);
addHandlerProps(Document.prototype);

// `implementation.createHTMLDocument` / `DOMParser`: a detached subtree.
class DetachedDocument {
  constructor(html) {
    this._root = document.createElement('html');
    this._root.innerHTML = html === undefined ? '<head></head><body></body>' : html;
    if (!this._root.querySelector('body')) { const b = document.createElement('body'); b.innerHTML = this._root.innerHTML; this._root.innerHTML = '<head></head>'; this._root.appendChild(b); }
  }
  get documentElement() { return this._root; }
  get head() { return this._root.querySelector('head'); }
  get body() { return this._root.querySelector('body'); }
  get title() { const t = this._root.querySelector('title'); return t ? t.textContent : ''; }
  get nodeType() { return 9; }
  querySelector(s) { return this._root.querySelector(s); }
  querySelectorAll(s) { return this._root.querySelectorAll(s); }
  getElementById(id) { return this._root.querySelector('#' + cssEscapeIdent(id)); }
  getElementsByTagName(t) { return this._root.getElementsByTagName(t); }
  getElementsByClassName(c) { return this._root.getElementsByClassName(c); }
  createElement(t) { return document.createElement(t); }
  createTextNode(t) { return document.createTextNode(t); }
  createDocumentFragment() { return document.createDocumentFragment(); }
  importNode(n, d) { return n.cloneNode(d); }
  adoptNode(n) { return n; }
}
class DOMParser {
  parseFromString(s) { return new DetachedDocument(String(s)); }
}

class TreeWalker {
  constructor(root, what, filter) { this.root = root; this.whatToShow = what === undefined ? 0xFFFFFFFF : what; this.filter = filter || null; this.currentNode = root; this._list = null; this._i = 0; }
  _all() {
    if (this._list) return this._list;
    const out = [];
    const walk = n => { for (const c of n.childNodes) { const t = c.nodeType; if (this.whatToShow & (1 << (t - 1))) { const f = this.filter; const r = !f ? 1 : typeof f === 'function' ? f(c) : f.acceptNode(c); if (r === 1) out.push(c); if (r === 2) continue; } walk(c); } };
    walk(this.root);
    return (this._list = out);
  }
  nextNode() { const l = this._all(); const n = l[this._i++]; if (n) this.currentNode = n; return n || null; }
  previousNode() { const l = this._all(); this._i = Math.max(0, this._i - 2); const n = l[this._i++]; return n || null; }
  firstChild() { return this.nextNode(); }
  parentNode() { const p = this.currentNode.parentNode; if (p && this.root.contains(p)) { this.currentNode = p; return p; } return null; }
  nextSibling() { const s = this.currentNode.nextSibling; if (s) this.currentNode = s; return s; }
  detach() {}
}
const NodeFilter = { FILTER_ACCEPT: 1, FILTER_REJECT: 2, FILTER_SKIP: 3, SHOW_ALL: 0xFFFFFFFF, SHOW_ELEMENT: 1, SHOW_TEXT: 4, SHOW_COMMENT: 128 };

class Range {
  constructor() { this.startContainer = null; this.endContainer = null; this.startOffset = 0; this.endOffset = 0; this.collapsed = true; this.commonAncestorContainer = null; }
  setStart(n, o) { this.startContainer = n; this.startOffset = o; this.commonAncestorContainer = n; }
  setEnd(n, o) { this.endContainer = n; this.endOffset = o; this.collapsed = false; }
  setStartBefore(n) { this.setStart(n.parentNode, 0); }
  setEndAfter(n) { this.setEnd(n.parentNode, 0); }
  selectNode(n) { this.setStart(n, 0); this.setEnd(n, 0); }
  selectNodeContents(n) { this.setStart(n, 0); this.setEnd(n, 0); }
  collapse() { this.collapsed = true; }
  cloneRange() { return Object.assign(new Range(), this); }
  detach() {}
  getBoundingClientRect() { return this.startContainer && this.startContainer.getBoundingClientRect ? this.startContainer.getBoundingClientRect() : new DOMRect(); }
  getClientRects() { return []; }
  createContextualFragment(html) { const f = document.createDocumentFragment(); H.setInnerHTML(f[ID], String(html)); return f; }
  deleteContents() {}
  insertNode(n) { if (this.startContainer) this.startContainer.appendChild(n); }
  toString() { return ''; }
}

const fontSet = {
  ready: Promise.resolve(),
  status: 'loaded',
  load() { return Promise.resolve([]); },
  check() { return true; },
  add() {}, delete() {}, clear() {},
  addEventListener() {}, removeEventListener() {},
  forEach() {},
  [Symbol.iterator]() { return [][Symbol.iterator](); },
};

const document = Object.create(Document.prototype);
def(document, { [ID]: 0 });
wrappers.set(0, document);

// ── Window ──────────────────────────────────────────────────────────────────
class Window extends EventTarget {}
Object.setPrototypeOf(G, Window.prototype);
addHandlerProps(Window.prototype);

// Navigation: queued to the browser (honoured only when applied live).
function navigateTo(href, replace) {
  const u = H.urlParse(String(href), document.baseURI);
  if (!u) throw domErr('SyntaxError', 'invalid URL: ' + href);
  if (u[1] === 'javascript:') { try { (0, eval)(decodeURIComponent(u[0].slice(11))); } catch (e) { reportError(e); } return; }
  const cur = H.urlParse(locHref);
  if (cur && u[0].split('#')[0] === cur[0].split('#')[0] && u[9]) {
    // Fragment-only change: scroll there, no reload.
    const old = locHref;
    locHref = u[0];
    const id = decodeURIComponent(u[9].slice(1));
    const t = document.getElementById(id) || document.querySelector('a[name="' + id.replace(/"/g, '\\"') + '"]');
    if (t) t.scrollIntoView();
    fire(G, 'hashchange', HashChangeEvent, { oldURL: old, newURL: locHref });
    return;
  }
  H.navigate(u[0], !!replace);
}

let locHref = H.url();
const LOC_PARTS = ['href', 'protocol', 'username', 'password', 'host', 'hostname', 'port', 'pathname', 'search', 'hash', 'origin'];
const location = {};
LOC_PARTS.forEach((k, i) => {
  getter(location, k, () => { const u = H.urlParse(locHref); return u ? u[i] : ''; }, k === 'origin' ? undefined : v => {
    if (k === 'href') { navigateTo(v, false); return; }
    const u = new URL(locHref);
    u[k] = v;
    navigateTo(u.href, false);
  });
});
def(location, {
  assign(u) { navigateTo(u, false); },
  replace(u) { navigateTo(u, true); },
  reload() { H.navigate(locHref, true); },
  toString() { return locHref; },
  valueOf() { return this; },
  ancestorOrigins: [],
});

const historyStates = [{ state: null, url: locHref }];
let historyIndex = 0;
const history = {
  get length() { return historyStates.length; },
  get state() { return historyStates[historyIndex].state; },
  scrollRestoration: 'auto',
  pushState(state, _title, url) {
    historyStates.length = historyIndex + 1;
    if (url !== undefined && url !== null) { const u = H.urlParse(String(url), locHref); if (u) locHref = u[0]; }
    historyStates.push({ state: clone(state), url: locHref });
    historyIndex++;
  },
  replaceState(state, _title, url) {
    if (url !== undefined && url !== null) { const u = H.urlParse(String(url), locHref); if (u) locHref = u[0]; }
    historyStates[historyIndex] = { state: clone(state), url: locHref };
  },
  go(n) {
    n = n | 0;
    const i = historyIndex + n;
    if (n === 0) { location.reload(); return; }
    if (i < 0 || i >= historyStates.length) return;
    historyIndex = i;
    locHref = historyStates[i].url;
    setTimeout(() => fire(G, 'popstate', PopStateEvent, { state: historyStates[i].state }), 0);
  },
  back() { this.go(-1); },
  forward() { this.go(1); },
};

function clone(v) {
  if (v === undefined) return null;
  try { return JSON.parse(JSON.stringify(v)); } catch (e) { return v; }
}

const navigator = {
  userAgent: 'Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0 Interstice',
  appName: 'Netscape', appVersion: '5.0 (X11)', appCodeName: 'Mozilla', product: 'Gecko', productSub: '20100101',
  vendor: '', vendorSub: '', platform: 'Linux x86_64', oscpu: 'Linux x86_64',
  language: 'en-US', languages: ['en-US', 'en'], onLine: true, cookieEnabled: true, doNotTrack: null,
  hardwareConcurrency: 1, maxTouchPoints: 0, deviceMemory: 4, webdriver: false, pdfViewerEnabled: false,
  plugins: [], mimeTypes: [],
  javaEnabled() { return false; },
  sendBeacon() { return true; },
  vibrate() { return false; },
  registerProtocolHandler() {},
  clipboard: { writeText() { return Promise.resolve(); }, readText() { return Promise.resolve(''); } },
  serviceWorker: undefined,
  permissions: { query() { return Promise.resolve({ state: 'denied', addEventListener() {} }); } },
  mediaDevices: undefined,
  storage: { estimate() { return Promise.resolve({ quota: 0, usage: 0 }); }, persist() { return Promise.resolve(false); } },
  userAgentData: undefined,
  connection: { effectiveType: '4g', downlink: 10, rtt: 50, saveData: false, addEventListener() {} },
};
delete navigator.serviceWorker;
delete navigator.mediaDevices;
delete navigator.userAgentData;

function vp() { return H.viewport(); }
const screen = {
  get width() { return vp()[0]; }, get height() { return vp()[1]; },
  get availWidth() { return vp()[0]; }, get availHeight() { return vp()[1]; },
  availLeft: 0, availTop: 0, colorDepth: 24, pixelDepth: 24,
  orientation: { type: 'landscape-primary', angle: 0, addEventListener() {}, removeEventListener() {} },
};

// ── Media queries ───────────────────────────────────────────────────────────
function px(v) {
  const m = /^\s*(-?[\d.]+)\s*(px|em|rem)?\s*$/.exec(v);
  if (!m) return NaN;
  return parseFloat(m[1]) * (m[2] === 'em' || m[2] === 'rem' ? 16 : 1);
}
function evalFeature(f) {
  const [w, h] = vp();
  const m = /^\s*([a-z-]+)\s*(?::\s*(.+?))?\s*$/.exec(f.toLowerCase());
  if (!m) return false;
  const [, name, val] = m;
  switch (name) {
    case 'min-width': return w >= px(val);
    case 'max-width': return w <= px(val);
    case 'width': return w === px(val);
    case 'min-height': return h >= px(val);
    case 'max-height': return h <= px(val);
    case 'height': return h === px(val);
    case 'orientation': return val === (w >= h ? 'landscape' : 'portrait');
    case 'prefers-color-scheme': return val === 'light';
    case 'prefers-reduced-motion': return val === 'no-preference';
    case 'prefers-contrast': return val === 'no-preference';
    case 'hover': return val === 'hover' || val === undefined;
    case 'any-hover': return val === 'hover' || val === undefined;
    case 'pointer': case 'any-pointer': return val === 'fine' || val === undefined;
    case 'color': return true;
    case 'min-resolution': return parseFloat(val) <= 1 || /96dpi/.test(val);
    case 'max-resolution': return true;
    case '-webkit-min-device-pixel-ratio': case 'min-device-pixel-ratio': return parseFloat(val) <= 1;
    case '-webkit-max-device-pixel-ratio': case 'max-device-pixel-ratio': return parseFloat(val) >= 1;
    case 'display-mode': return val === 'browser';
    case 'forced-colors': return val === 'none';
    case 'scripting': return val === 'enabled';
    case 'update': return val === 'fast';
    default: return false;
  }
}
function evalMedia(q) {
  q = String(q).trim();
  if (!q) return true;
  return q.split(',').some(part => {
    part = part.trim().toLowerCase();
    let neg = false;
    if (part.startsWith('not ')) { neg = true; part = part.slice(4); }
    part = part.replace(/^only\s+/, '');
    let ok = true;
    for (const term of part.split(/\s+and\s+/)) {
      const t = term.trim();
      if (!t) continue;
      if (t === 'screen' || t === 'all') continue;
      if (t === 'print' || t === 'speech') { ok = false; continue; }
      const f = /^\((.*)\)$/.exec(t);
      if (!f || !evalFeature(f[1])) ok = false;
    }
    return neg ? !ok : ok;
  });
}
class MediaQueryList extends EventTarget {
  constructor(q) { super(); this.media = String(q); this.onchange = null; }
  get matches() { return evalMedia(this.media); }
  addListener(f) { this.addEventListener('change', f); }
  removeListener(f) { this.removeEventListener('change', f); }
}

// ── Timers ──────────────────────────────────────────────────────────────────
// Virtual time: `performance.now()` advances only when the browser applies a
// tick event, so timers fire identically when a page is replayed.
const timerQ = new Map();
let timerSeq = 1;
const rafQ = new Map();
let rafSeq = 1;
function addTimer(cb, ms, args, repeat) {
  if (typeof cb !== 'function') { const src = str(cb); cb = () => (0, eval)(src); }
  const id = timerSeq++;
  const delay = Math.max(0, +ms || 0);
  timerQ.set(id, { cb, args, due: H.now() + Math.max(delay, repeat ? 4 : 0), every: repeat ? Math.max(delay, 4) : -1, seq: id });
  return id;
}
function runTimers(now) {
  // Run in due order; timers added while running wait for the next tick.
  const due = Array.from(timerQ.entries()).filter(([, t]) => t.due <= now).sort((a, b) => a[1].due - b[1].due || a[1].seq - b[1].seq);
  for (const [id, t] of due) {
    if (!timerQ.has(id)) continue;
    if (t.every >= 0) t.due = Math.max(t.due + t.every, now + 1); else timerQ.delete(id);
    try { t.cb.apply(G, t.args); } catch (e) { reportError(e); }
    drainMicrotasks();
  }
}
function runFrames(now) {
  if (!rafQ.size) return;
  const cbs = Array.from(rafQ.values());
  rafQ.clear();
  for (const cb of cbs) { try { cb(now); } catch (e) { reportError(e); } }
}
function drainMicrotasks() {}

// ── Storage ─────────────────────────────────────────────────────────────────
// `localStorage` is backed by the browser's `WebStorage` table (per origin);
// `sessionStorage` lives as long as the realm.
function makeStorage(backend) {
  const target = {
    getItem(k) { return backend.get(String(k)); },
    setItem(k, v) { backend.set(String(k), String(v)); },
    removeItem(k) { backend.set(String(k), null); },
    clear() { for (const k of backend.keys()) backend.set(k, null); },
    key(i) { return backend.keys()[i] || null; },
    get length() { return backend.keys().length; },
  };
  return new Proxy(target, {
    get(t, p) { if (typeof p !== 'string' || p in t) return Reflect.get(t, p); const v = backend.get(p); return v === null ? undefined : v; },
    set(t, p, v) { if (typeof p !== 'string' || p in t) return false; backend.set(p, String(v)); return true; },
    deleteProperty(t, p) { backend.set(String(p), null); return true; },
    has(t, p) { return p in t || backend.get(String(p)) !== null; },
    ownKeys() { return backend.keys(); },
    getOwnPropertyDescriptor(t, p) { const v = backend.get(String(p)); return v === null ? undefined : { value: v, enumerable: true, configurable: true, writable: true }; },
  });
}
const localStorage = makeStorage({ get: k => H.storageGet(k), set: (k, v) => H.storageSet(k, v), keys: () => H.storageKeys() });
const sessionMap = new Map();
const sessionStorage = makeStorage({ get: k => (sessionMap.has(k) ? sessionMap.get(k) : null), set: (k, v) => { if (v === null) sessionMap.delete(k); else sessionMap.set(k, v); }, keys: () => Array.from(sessionMap.keys()) });

// ── URL ─────────────────────────────────────────────────────────────────────
const URL_KEYS = ['href', 'protocol', 'username', 'password', 'host', 'hostname', 'port', 'pathname', 'search', 'hash', 'origin'];
class URL {
  constructor(url, base) {
    const p = base === undefined ? H.urlParse(String(url)) : H.urlParse(String(url), String(base instanceof URL ? base.href : base));
    if (!p) throw new TypeError('Invalid URL: ' + url);
    def(this, { _p: p, _sp: null });
  }
  static canParse(u, b) { try { new URL(u, b); return true; } catch (e) { return false; } }
  static parse(u, b) { try { return new URL(u, b); } catch (e) { return null; } }
  static createObjectURL() { return 'blob:null/0'; }
  static revokeObjectURL() {}
  get searchParams() {
    if (!this._sp) { this._sp = new URLSearchParams(this.search); this._sp._url = this; }
    return this._sp;
  }
  toString() { return this.href; }
  toJSON() { return this.href; }
  _rebuild(parts) {
    const [href, protocol, username, password, host, , , pathname, search, hash] = parts;
    const auth = username ? username + (password ? ':' + password : '') + '@' : '';
    const special = /^(https?|ftp|wss?|file):$/.test(protocol);
    const s = protocol + (special || host ? '//' : '') + auth + host + pathname + search + hash;
    const p = H.urlParse(s) || H.urlParse(href);
    this._p = p;
    if (this._sp) { const sp = new URLSearchParams(this.search); this._sp._list = sp._list; }
  }
}
URL_KEYS.forEach((k, i) => {
  getter(URL.prototype, k, function () { return this._p[i]; }, k === 'origin' ? undefined : function (v) {
    v = String(v);
    if (k === 'href') { const p = H.urlParse(v); if (!p) throw new TypeError('Invalid URL'); this._p = p; if (this._sp) this._sp._list = new URLSearchParams(this.search)._list; return; }
    const parts = this._p.slice();
    if (k === 'protocol') parts[1] = v.replace(/:?$/, ':');
    else if (k === 'search') parts[8] = v && v !== '?' ? (v[0] === '?' ? v : '?' + v) : '';
    else if (k === 'hash') parts[9] = v && v !== '#' ? (v[0] === '#' ? v : '#' + v) : '';
    else if (k === 'pathname') parts[7] = v[0] === '/' ? v : '/' + v;
    else if (k === 'host') parts[4] = v;
    else if (k === 'hostname') parts[4] = v + (parts[6] ? ':' + parts[6] : '');
    else if (k === 'port') parts[4] = parts[5] + (v ? ':' + v : '');
    else if (k === 'username') parts[2] = v;
    else if (k === 'password') parts[3] = v;
    this._rebuild(parts);
  });
});
G.webkitURL = URL;

function encodeQ(s) { return encodeURIComponent(s).replace(/%20/g, '+').replace(/[!'()~]/g, c => '%' + c.charCodeAt(0).toString(16).toUpperCase()); }
function decodeQ(s) { try { return decodeURIComponent(s.replace(/\+/g, ' ')); } catch (e) { return s; } }
class URLSearchParams {
  constructor(init) {
    def(this, { _list: [], _url: null });
    if (init === undefined || init === null) return;
    if (typeof init === 'string') {
      for (const part of init.replace(/^\?/, '').split('&')) {
        if (!part) continue;
        const i = part.indexOf('=');
        this._list.push(i < 0 ? [decodeQ(part), ''] : [decodeQ(part.slice(0, i)), decodeQ(part.slice(i + 1))]);
      }
    } else if (init instanceof FormData || init instanceof URLSearchParams) {
      for (const [k, v] of init) this._list.push([k, String(v)]);
    } else if (typeof init[Symbol.iterator] === 'function') {
      for (const [k, v] of init) this._list.push([String(k), String(v)]);
    } else {
      for (const k of Object.keys(init)) this._list.push([k, String(init[k])]);
    }
  }
  _update() { if (this._url) { const s = this.toString(); const parts = this._url._p.slice(); parts[8] = s ? '?' + s : ''; const sp = this._url._sp; this._url._rebuild(parts); this._url._sp = sp; } }
  append(k, v) { this._list.push([String(k), String(v)]); this._update(); }
  delete(k, v) { this._list = this._list.filter(([a, b]) => a !== String(k) || (v !== undefined && b !== String(v))); this._update(); }
  get(k) { const e = this._list.find(([a]) => a === String(k)); return e ? e[1] : null; }
  getAll(k) { return this._list.filter(([a]) => a === String(k)).map(e => e[1]); }
  has(k, v) { return this._list.some(([a, b]) => a === String(k) && (v === undefined || b === String(v))); }
  set(k, v) {
    k = String(k);
    const i = this._list.findIndex(([a]) => a === k);
    if (i < 0) this._list.push([k, String(v)]);
    else { this._list[i][1] = String(v); this._list = this._list.filter(([a], j) => a !== k || j === i); }
    this._update();
  }
  sort() { this._list.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0)); this._update(); }
  forEach(f, t) { for (const [k, v] of this._list) f.call(t, v, k, this); }
  keys() { return this._list.map(e => e[0])[Symbol.iterator](); }
  values() { return this._list.map(e => e[1])[Symbol.iterator](); }
  entries() { return this._list.map(e => [e[0], e[1]])[Symbol.iterator](); }
  [Symbol.iterator]() { return this.entries(); }
  get size() { return this._list.length; }
  toString() { return this._list.map(([k, v]) => encodeQ(k) + '=' + encodeQ(v)).join('&'); }
}

class FormData {
  constructor(form) {
    def(this, { _list: [] });
    if (!form) return;
    for (const e of form.elements) {
      const n = e.name;
      if (!n || e.disabled) continue;
      const t = e.type;
      if ((t === 'checkbox' || t === 'radio') && !e.checked) continue;
      if (t === 'submit' || t === 'button' || t === 'reset' || t === 'file' || t === 'image' || e.localName === 'fieldset' || e.localName === 'output') continue;
      if (e.localName === 'select') { for (const o of e.selectedOptions) this._list.push([n, o.value]); continue; }
      this._list.push([n, e.value]);
    }
  }
  append(k, v) { this._list.push([String(k), v]); }
  delete(k) { this._list = this._list.filter(([a]) => a !== k); }
  get(k) { const e = this._list.find(([a]) => a === k); return e ? e[1] : null; }
  getAll(k) { return this._list.filter(([a]) => a === k).map(e => e[1]); }
  has(k) { return this._list.some(([a]) => a === k); }
  set(k, v) { this.delete(k); this.append(k, v); }
  forEach(f, t) { for (const [k, v] of this._list) f.call(t, v, k, this); }
  keys() { return this._list.map(e => e[0])[Symbol.iterator](); }
  values() { return this._list.map(e => e[1])[Symbol.iterator](); }
  entries() { return this._list.map(e => [e[0], e[1]])[Symbol.iterator](); }
  [Symbol.iterator]() { return this.entries(); }
}

// ── Encoding ────────────────────────────────────────────────────────────────
class TextEncoder {
  get encoding() { return 'utf-8'; }
  encode(s) {
    s = str(s);
    const out = [];
    for (const ch of s) {
      let c = ch.codePointAt(0);
      if (c >= 0xD800 && c <= 0xDFFF) c = 0xFFFD;
      if (c < 0x80) out.push(c);
      else if (c < 0x800) out.push(0xC0 | (c >> 6), 0x80 | (c & 63));
      else if (c < 0x10000) out.push(0xE0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
      else out.push(0xF0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
    }
    return new Uint8Array(out);
  }
  encodeInto(s, dst) { const b = this.encode(s); const n = Math.min(b.length, dst.length); dst.set(b.subarray(0, n)); return { read: s.length, written: n }; }
}
class TextDecoder {
  constructor(label) { this.encoding = (label || 'utf-8').toLowerCase(); }
  decode(buf) {
    if (!buf) return '';
    const b = buf instanceof Uint8Array ? buf : ArrayBuffer.isView(buf) ? new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength) : new Uint8Array(buf);
    if (this.encoding !== 'utf-8' && this.encoding !== 'utf8') return String.fromCharCode.apply(null, Array.from(b));
    let s = '';
    for (let i = 0; i < b.length;) {
      const c = b[i];
      let cp, n;
      if (c < 0x80) { cp = c; n = 1; }
      else if (c >> 5 === 6) { cp = c & 31; n = 2; }
      else if (c >> 4 === 14) { cp = c & 15; n = 3; }
      else if (c >> 3 === 30) { cp = c & 7; n = 4; }
      else { s += '�'; i++; continue; }
      for (let j = 1; j < n; j++) cp = (cp << 6) | ((b[i + j] || 0) & 63);
      s += String.fromCodePoint(cp > 0x10FFFF ? 0xFFFD : cp);
      i += n;
    }
    return s;
  }
}
const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
function btoa(s) {
  s = str(s);
  let out = '';
  for (let i = 0; i < s.length; i += 3) {
    const a = s.charCodeAt(i), b = s.charCodeAt(i + 1), c = s.charCodeAt(i + 2);
    if (a > 255 || b > 255 || c > 255) throw domErr('InvalidCharacterError', 'btoa: non-latin1');
    const n = (a << 16) | ((b || 0) << 8) | (c || 0);
    out += B64[(n >> 18) & 63] + B64[(n >> 12) & 63] + (i + 1 < s.length ? B64[(n >> 6) & 63] : '=') + (i + 2 < s.length ? B64[n & 63] : '=');
  }
  return out;
}
function atob(s) {
  s = str(s).replace(/[\s=]/g, '');
  let out = '', acc = 0, bits = 0;
  for (const ch of s) {
    const v = B64.indexOf(ch);
    if (v < 0) throw domErr('InvalidCharacterError', 'atob: invalid character');
    acc = (acc << 6) | v; bits += 6;
    if (bits >= 8) { bits -= 8; out += String.fromCharCode((acc >> bits) & 255); }
  }
  return out;
}

// ── Network ─────────────────────────────────────────────────────────────────
// GET only: each request becomes a fetch id; the browser completes it with a
// logged event once the body is in its resource cache.
const pendingFetches = new Map();
function hostFetch(url) {
  return new Promise((resolve, reject) => {
    const id = H.fetch(String(url));
    if (id < 0) { reject(new TypeError('Failed to fetch: unsupported URL ' + url)); return; }
    pendingFetches.set(id, { resolve, reject });
  });
}

class Headers {
  constructor(init) {
    def(this, { _m: new Map() });
    if (!init) return;
    if (init instanceof Headers) init.forEach((v, k) => this.append(k, v));
    else if (typeof init[Symbol.iterator] === 'function') for (const [k, v] of init) this.append(k, v);
    else for (const k of Object.keys(init)) this.append(k, init[k]);
  }
  append(k, v) { k = String(k).toLowerCase(); const o = this._m.get(k); this._m.set(k, o === undefined ? String(v) : o + ', ' + v); }
  set(k, v) { this._m.set(String(k).toLowerCase(), String(v)); }
  get(k) { const v = this._m.get(String(k).toLowerCase()); return v === undefined ? null : v; }
  has(k) { return this._m.has(String(k).toLowerCase()); }
  delete(k) { this._m.delete(String(k).toLowerCase()); }
  forEach(f, t) { for (const [k, v] of this._m) f.call(t, v, k, this); }
  keys() { return this._m.keys(); }
  values() { return this._m.values(); }
  entries() { return this._m.entries(); }
  [Symbol.iterator]() { return this._m.entries(); }
}

function guessType(url, body) {
  if (/\.json(\?|$)/.test(url) || /^\s*[{[]/.test(body)) return 'application/json';
  if (/\.js(\?|$)/.test(url)) return 'text/javascript';
  if (/\.css(\?|$)/.test(url)) return 'text/css';
  if (/^\s*</.test(body)) return 'text/html';
  return 'text/plain';
}

class Blob {
  constructor(parts, opts) { def(this, { _t: (parts || []).map(p => (p instanceof Blob ? p._t : typeof p === 'string' ? p : new TextDecoder().decode(p))).join('') }); this.type = (opts && opts.type) || ''; }
  get size() { return new TextEncoder().encode(this._t).length; }
  text() { return Promise.resolve(this._t); }
  arrayBuffer() { return Promise.resolve(new TextEncoder().encode(this._t).buffer); }
  slice(a, b) { return new Blob([this._t.slice(a, b)], { type: this.type }); }
}
class File extends Blob { constructor(parts, name, opts) { super(parts, opts); this.name = name; this.lastModified = H.timeOrigin() + H.now(); } }

class Response {
  constructor(body, init) {
    init = init || {};
    def(this, { _body: body === undefined || body === null ? '' : body instanceof Blob ? body._t : String(body), _used: false });
    this.status = init.status === undefined ? 200 : init.status;
    this.statusText = init.statusText || (this.status === 200 ? 'OK' : '');
    this.headers = new Headers(init.headers);
    this.url = init.url || '';
    this.redirected = false;
    this.type = 'basic';
  }
  get ok() { return this.status >= 200 && this.status < 300; }
  get bodyUsed() { return this._used; }
  get body() { return null; }
  _take() { if (this._used) return Promise.reject(new TypeError('body already used')); this._used = true; return Promise.resolve(this._body); }
  text() { return this._take(); }
  json() { return this._take().then(t => JSON.parse(t)); }
  arrayBuffer() { return this._take().then(t => new TextEncoder().encode(t).buffer); }
  bytes() { return this._take().then(t => new TextEncoder().encode(t)); }
  blob() { return this._take().then(t => new Blob([t], { type: this.headers.get('content-type') || '' })); }
  formData() { return this._take().then(t => { const f = new FormData(); for (const [k, v] of new URLSearchParams(t)) f.append(k, v); return f; }); }
  clone() { const r = new Response(this._body, { status: this.status, statusText: this.statusText, headers: this.headers, url: this.url }); return r; }
  static json(v, init) { return new Response(JSON.stringify(v), Object.assign({ headers: { 'content-type': 'application/json' } }, init)); }
  static error() { return new Response('', { status: 0 }); }
}
class Request {
  constructor(input, init) {
    init = init || {};
    this.url = input instanceof Request ? input.url : new URL(String(input), document.baseURI).href;
    this.method = (init.method || (input instanceof Request ? input.method : 'GET')).toUpperCase();
    this.headers = new Headers(init.headers || (input instanceof Request ? input.headers : undefined));
    this.signal = init.signal || null;
    this.body = init.body || null;
    this.credentials = init.credentials || 'same-origin';
    this.mode = init.mode || 'cors';
    this.cache = init.cache || 'default';
  }
  clone() { return new Request(this); }
}

function fetch(input, init) {
  let req;
  try { req = new Request(input, init); } catch (e) { return Promise.reject(e); }
  if (req.method !== 'GET' && req.method !== 'HEAD') return Promise.reject(new TypeError('fetch: ' + req.method + ' is not supported'));
  const sig = req.signal;
  if (sig && sig.aborted) return Promise.reject(sig.reason || domErr('AbortError', 'aborted'));
  const p = hostFetch(req.url).then(({ status, text, url }) => {
    if (status === 0) throw new TypeError('Failed to fetch');
    return new Response(text, { status, url, headers: { 'content-type': guessType(url, text) } });
  });
  if (!sig) return p;
  return new Promise((resolve, reject) => {
    sig.addEventListener('abort', () => reject(sig.reason || domErr('AbortError', 'aborted')));
    p.then(resolve, reject);
  });
}

class XMLHttpRequest extends EventTarget {
  constructor() {
    super();
    this.readyState = 0; this.status = 0; this.statusText = ''; this.responseText = ''; this.responseXML = null;
    this.responseType = ''; this.responseURL = ''; this.timeout = 0; this.withCredentials = false;
    this.upload = new EventTarget();
    def(this, { _url: '', _method: 'GET', _aborted: false });
  }
  get response() {
    if (this.responseType === 'json') { try { return JSON.parse(this.responseText); } catch (e) { return null; } }
    if (this.responseType === 'arraybuffer') return new TextEncoder().encode(this.responseText).buffer;
    if (this.responseType === 'blob') return new Blob([this.responseText]);
    if (this.responseType === 'document') return new DetachedDocument(this.responseText);
    return this.responseText;
  }
  open(method, url) { this._method = String(method).toUpperCase(); this._url = new URL(String(url), document.baseURI).href; this._aborted = false; this._state(1); }
  setRequestHeader() {}
  overrideMimeType() {}
  getResponseHeader(n) { return String(n).toLowerCase() === 'content-type' && this.readyState >= 2 ? guessType(this._url, this.responseText) : null; }
  getAllResponseHeaders() { return this.readyState >= 2 ? 'content-type: ' + guessType(this._url, this.responseText) + '\r\n' : ''; }
  abort() { this._aborted = true; this._ev('abort'); this._ev('loadend'); }
  _state(s) { this.readyState = s; this._ev('readystatechange'); }
  _ev(t) { const e = new ProgressEvent(t, {}); this.dispatchEvent(e); }
  send() {
    const fail = () => { if (this._aborted) return; this.status = 0; this._state(4); this._ev('error'); this._ev('loadend'); };
    if (this._method !== 'GET' && this._method !== 'HEAD') { console.warn('XMLHttpRequest: ' + this._method + ' is not supported'); setTimeout(fail, 0); return; }
    this._ev('loadstart');
    hostFetch(this._url).then(({ status, text, url }) => {
      if (this._aborted) return;
      if (status === 0) { fail(); return; }
      this.status = status; this.statusText = 'OK'; this.responseURL = url;
      this._state(2); this._state(3);
      this.responseText = text;
      this._state(4);
      this._ev('load'); this._ev('loadend');
    }, fail);
  }
}
Object.assign(XMLHttpRequest, { UNSENT: 0, OPENED: 1, HEADERS_RECEIVED: 2, LOADING: 3, DONE: 4 });
addHandlerProps(XMLHttpRequest.prototype);

class AbortSignal extends EventTarget {
  constructor() { super(); this.aborted = false; this.reason = undefined; this.onabort = null; }
  throwIfAborted() { if (this.aborted) throw this.reason; }
  static abort(r) { const c = new AbortController(); c.abort(r); return c.signal; }
  static timeout(ms) { const c = new AbortController(); setTimeout(() => c.abort(domErr('TimeoutError', 'timeout')), ms); return c.signal; }
  static any(sigs) { const c = new AbortController(); for (const s of sigs) { if (s.aborted) { c.abort(s.reason); break; } s.addEventListener('abort', () => c.abort(s.reason)); } return c.signal; }
}
class AbortController {
  constructor() { this.signal = new AbortSignal(); }
  abort(reason) {
    const s = this.signal;
    if (s.aborted) return;
    s.aborted = true;
    s.reason = reason === undefined ? domErr('AbortError', 'signal is aborted without reason') : reason;
    const e = new Event('abort');
    if (typeof s.onabort === 'function') { try { s.onabort(e); } catch (x) { reportError(x); } }
    s.dispatchEvent(e);
  }
}

// ── Observers & misc stubs ──────────────────────────────────────────────────
class MutationObserver { constructor(cb) { this._cb = cb; } observe() {} disconnect() {} takeRecords() { return []; } }
class ResizeObserver {
  constructor(cb) { this._cb = cb; this._t = []; }
  observe(t) {
    this._t.push(t);
    setTimeout(() => {
      const r = t.getBoundingClientRect();
      const e = { target: t, contentRect: r, borderBoxSize: [{ inlineSize: r.width, blockSize: r.height }], contentBoxSize: [{ inlineSize: r.width, blockSize: r.height }] };
      try { this._cb([e], this); } catch (x) { reportError(x); }
    }, 0);
  }
  unobserve(t) { this._t = this._t.filter(x => x !== t); }
  disconnect() { this._t = []; }
}
// Reports every target as intersecting once (lazy loaders then load).
class IntersectionObserver {
  constructor(cb, opts) { this._cb = cb; this.root = (opts && opts.root) || null; this.rootMargin = (opts && opts.rootMargin) || '0px'; this.thresholds = [0]; }
  observe(t) {
    setTimeout(() => {
      const r = t.getBoundingClientRect();
      const e = { target: t, isIntersecting: true, intersectionRatio: 1, boundingClientRect: r, intersectionRect: r, rootBounds: null, time: H.now() };
      try { this._cb([e], this); } catch (x) { reportError(x); }
    }, 0);
  }
  unobserve() {}
  disconnect() {}
  takeRecords() { return []; }
}
class PerformanceObserver { constructor() {} observe() {} disconnect() {} takeRecords() { return []; } }
PerformanceObserver.supportedEntryTypes = [];

const customElements = {
  define(name, cls) { customElementClasses.set(String(name), cls); for (const el of document.querySelectorAll(name)) { wrappers.delete(el[ID]); const w = wrap(el[ID]); if (typeof w.connectedCallback === 'function') { try { w.connectedCallback(); } catch (e) { reportError(e); } } } },
  get(name) { return customElementClasses.get(String(name)); },
  whenDefined(name) { return customElementClasses.has(name) ? Promise.resolve(customElementClasses.get(name)) : new Promise(() => {}); },
  upgrade() {},
};

const selection = {
  rangeCount: 0, isCollapsed: true, type: 'None', anchorNode: null, focusNode: null, anchorOffset: 0, focusOffset: 0,
  getRangeAt() { return new Range(); }, removeAllRanges() {}, addRange() {}, collapse() {}, extend() {},
  selectAllChildren() {}, toString() { return ''; }, containsNode() { return false; }, empty() {},
};

const performance = {
  now() { return H.now(); },
  get timeOrigin() { return H.timeOrigin(); },
  timing: { navigationStart: H.timeOrigin(), fetchStart: H.timeOrigin(), domLoading: H.timeOrigin(), responseEnd: H.timeOrigin() },
  navigation: { type: 0, redirectCount: 0 },
  mark() {}, measure() {}, clearMarks() {}, clearMeasures() {},
  getEntries() { return []; }, getEntriesByType() { return []; }, getEntriesByName() { return []; },
  setResourceTimingBufferSize() {}, addEventListener() {}, removeEventListener() {},
  toJSON() { return {}; },
};

const crypto = {
  getRandomValues(a) {
    const bytes = new Uint8Array(a.buffer, a.byteOffset, a.byteLength);
    for (let i = 0; i < bytes.length; i++) bytes[i] = Math.floor(H.random() * 256);
    return a;
  },
  randomUUID() {
    const b = crypto.getRandomValues(new Uint8Array(16));
    b[6] = (b[6] & 15) | 64; b[8] = (b[8] & 63) | 128;
    const h = Array.from(b, x => x.toString(16).padStart(2, '0')).join('');
    return h.slice(0, 8) + '-' + h.slice(8, 12) + '-' + h.slice(12, 16) + '-' + h.slice(16, 20) + '-' + h.slice(20);
  },
  subtle: undefined,
};

// ── Globals ─────────────────────────────────────────────────────────────────
const globals = {
  document, location, history, navigator, screen, performance, localStorage, sessionStorage, crypto, customElements,
  EventTarget, Event, UIEvent, MouseEvent, PointerEvent, WheelEvent, KeyboardEvent, FocusEvent, InputEvent, CustomEvent,
  ErrorEvent, ProgressEvent, MessageEvent, PopStateEvent, HashChangeEvent, StorageEvent, TouchEvent, AnimationEvent,
  TransitionEvent, SubmitEvent,
  Node, CharacterData, Text, Comment, Element, HTMLElement, Document, HTMLDocument: Document, DocumentFragment, Attr, NodeList,
  HTMLCollection, DOMTokenList, CSSStyleDeclaration, DOMRect, DOMRectReadOnly: DOMRect, Range, TreeWalker, NodeFilter, DOMParser,
  HTMLAnchorElement, HTMLAreaElement, HTMLInputElement, HTMLTextAreaElement, HTMLButtonElement, HTMLSelectElement,
  HTMLOptionElement, HTMLFormElement, HTMLLabelElement, HTMLFieldSetElement, HTMLOutputElement, HTMLImageElement,
  HTMLScriptElement, HTMLLinkElement, HTMLStyleElement, HTMLMetaElement, HTMLBaseElement, HTMLTitleElement,
  HTMLIFrameElement, HTMLCanvasElement, HTMLMediaElement, HTMLVideoElement, HTMLAudioElement, HTMLTemplateElement,
  HTMLDetailsElement, HTMLDialogElement, HTMLTableElement, HTMLTableRowElement, HTMLTableCellElement,
  HTMLTableSectionElement, HTMLLIElement, HTMLOListElement, HTMLUListElement, HTMLProgressElement, HTMLDivElement,
  HTMLSpanElement, HTMLParagraphElement, HTMLHeadingElement, HTMLBodyElement, HTMLHeadElement, HTMLHtmlElement,
  HTMLPreElement, HTMLBRElement, HTMLHRElement, HTMLSourceElement, HTMLPictureElement, HTMLUnknownElement, HTMLSlotElement,
  SVGElement, SVGSVGElement, Window, MediaQueryList,
  URL, URLSearchParams, FormData, Headers, Request, Response, Blob, File, TextEncoder, TextDecoder, AbortController,
  AbortSignal, XMLHttpRequest, MutationObserver, ResizeObserver, IntersectionObserver, PerformanceObserver,
  fetch, atob, btoa,
  window: G, self: G, top: G, parent: G, frames: G, globalThis: G, opener: null, frameElement: null, closed: false,
  name: '', status: '', origin: H.urlParse(H.url()) ? H.urlParse(H.url())[10] : 'null', isSecureContext: true, length: 0,
  devicePixelRatio: 1,
  Image: function Image(w, h) { const i = document.createElement('img'); if (w !== undefined) i.width = w; if (h !== undefined) i.height = h; return i; },
  Option: function Option(text, value, ds, sel) { const o = document.createElement('option'); if (text !== undefined) o.text = text; if (value !== undefined) o.value = value; if (sel) o.selected = true; return o; },
  Audio: function Audio(src) { const a = document.createElement('audio'); if (src) a.src = src; return a; },
  CSS: { supports(p, v) { return v === undefined ? /\(|:/.test(String(p)) : true; }, escape: cssEscapeIdent },
  setTimeout(cb, ms, ...args) { return addTimer(cb, ms, args, false); },
  setInterval(cb, ms, ...args) { return addTimer(cb, ms, args, true); },
  clearTimeout(id) { timerQ.delete(id); },
  clearInterval(id) { timerQ.delete(id); },
  setImmediate(cb, ...args) { return addTimer(cb, 0, args, false); },
  requestAnimationFrame(cb) { const id = rafSeq++; rafQ.set(id, cb); return id; },
  cancelAnimationFrame(id) { rafQ.delete(id); },
  requestIdleCallback(cb) { return addTimer(() => cb({ didTimeout: false, timeRemaining: () => 50 }), 1, [], false); },
  cancelIdleCallback(id) { timerQ.delete(id); },
  queueMicrotask(cb) { Promise.resolve().then(cb).catch(reportError); },
  structuredClone(v) { return clone(v); },
  reportError,
  getComputedStyle(el, pseudo) {
    const id = el && el[ID];
    const read = p => (id === undefined ? '' : H.computed(id, camelToKebab(p)));
    return new Proxy({ getPropertyValue: read, getPropertyPriority: () => '', length: 0, item: () => '', cssText: '' }, {
      get(t, p) { if (typeof p !== 'string' || p in t) return t[p]; return read(p); },
    });
  },
  matchMedia(q) { return new MediaQueryList(q); },
  getSelection() { return selection; },
  scrollTo(x, y) { if (x && typeof x === 'object') { y = x.top; x = x.left; } H.scrollTo(-1, x === undefined ? NaN : +x, y === undefined ? NaN : +y); },
  scroll(x, y) { G.scrollTo(x, y); },
  scrollBy(x, y) { if (x && typeof x === 'object') { y = x.top; x = x.left; } G.scrollTo(G.scrollX + (+x || 0), G.scrollY + (+y || 0)); },
  alert(m) { H.log('alert', str(m)); },
  confirm(m) { H.log('confirm', str(m)); return false; },
  prompt(m) { H.log('prompt', str(m)); return null; },
  print() {},
  focus() {}, blur() {}, close() {}, stop() {},
  moveTo() {}, moveBy() {}, resizeTo() {}, resizeBy() {},
  open(url) { if (url) navigateTo(url, false); return null; },
  postMessage(data) { setTimeout(() => fire(G, 'message', MessageEvent, { data, origin: location.origin, source: G }), 0); },
  captureEvents() {}, releaseEvents() {},
};
for (const k of Object.keys(globals)) {
  Object.defineProperty(G, k, { value: globals[k], writable: true, configurable: true, enumerable: false });
}
for (const [k, fn] of [['innerWidth', () => vp()[0]], ['innerHeight', () => vp()[1]], ['outerWidth', () => vp()[0]], ['outerHeight', () => vp()[1]],
  ['scrollX', () => H.scrollPos(-1)[0]], ['scrollY', () => H.scrollPos(-1)[1]], ['pageXOffset', () => H.scrollPos(-1)[0]],
  ['pageYOffset', () => H.scrollPos(-1)[1]], ['screenX', () => 0], ['screenY', () => 0], ['screenLeft', () => 0], ['screenTop', () => 0],
  ['event', () => undefined]]) {
  Object.defineProperty(G, k, { get: fn, set(v) { Object.defineProperty(G, k, { value: v, writable: true, configurable: true }); }, configurable: true });
}
// Elements with an id are reachable as globals (`window.foo`) — a lookup on
// demand would need a proxy global, so only ids present at boot are bound.
function bindNamedElements() {
  for (const id of H.queryAll(0, '[id]')) {
    const name = H.getAttr(id, 'id');
    if (name && /^[A-Za-z_$][\w$]*$/.test(name) && !(name in G)) {
      Object.defineProperty(G, name, { get: () => document.getElementById(name), set(v) { Object.defineProperty(G, name, { value: v, writable: true, configurable: true }); }, configurable: true });
    }
  }
}

// ── Script loading ──────────────────────────────────────────────────────────
const JS_TYPES = /^(|text\/javascript|application\/javascript|application\/x-javascript|text\/ecmascript|application\/ecmascript|module|text\/jscript)$/i;
const loader = { started: false, ran: new Set(), queue: [], deferred: [], asyncPending: 0, dcl: false, loaded: false };

function isJs(el) {
  const t = str(H.getAttr(el, 'type')).trim().toLowerCase();
  const l = str(H.getAttr(el, 'language')).trim();
  if (t === 'module') return false; // ES modules are not supported.
  if (H.getAttr(el, 'nomodule') !== null) return true;
  return JS_TYPES.test(t.split(';')[0]) && (!l || /^javascript/i.test(l) || t);
}

function execScript(id, text, url) {
  const el = wrap(id);
  const prev = currentScript;
  currentScript = el;
  try {
    (0, eval)(text + (url ? '\n//# sourceURL=' + url : ''));
  } catch (e) {
    console.error('script ' + (url || '(inline)') + ': ' + (e && e.name ? e.name + ': ' + e.message : e));
    fire(G, 'error', ErrorEvent, { message: e && e.message, error: e, filename: url || '' });
  }
  currentScript = prev;
}

function load(entry) {
  entry.state = 'loading';
  hostFetch(entry.src).then(({ status, text }) => {
    entry.state = status === 0 ? 'failed' : 'ready';
    entry.text = text;
    if (entry.async) runAsync(entry); else pump();
  }, () => { entry.state = 'failed'; if (entry.async) runAsync(entry); else pump(); });
}

function entryFor(id) {
  loader.ran.add(id);
  const src = H.getAttr(id, 'src');
  const e = { id, src: src === null ? null : wrap(id).src, async: false, state: src === null ? 'ready' : 'idle', text: src === null ? H.text(id) : '' };
  return e;
}

function runAsync(e) {
  loader.asyncPending--;
  if (e.state === 'ready') { execScript(e.id, e.text, e.src); fire(wrap(e.id), 'load'); }
  else fire(wrap(e.id), 'error');
  checkLoaded();
}

// Parser-inserted scripts: in order (sync, then deferred), async on arrival.
function pump() {
  for (;;) {
    const q = loader.queue.length ? loader.queue : loader.dcl ? [] : loader.deferred;
    if (!q.length) break;
    const e = q[0];
    if (e.state === 'idle') load(e);
    if (e.state === 'loading') return;
    q.shift();
    if (e.state === 'ready') {
      execScript(e.id, e.text, e.src);
      if (e.src) fire(wrap(e.id), 'load');
    } else if (e.src) {
      fire(wrap(e.id), 'error');
    }
  }
  if (!loader.dcl) {
    loader.dcl = true;
    readyState = 'interactive';
    fire(document, 'readystatechange');
    fire(document, 'DOMContentLoaded', Event, { bubbles: true });
  }
  checkLoaded();
}

function checkLoaded() {
  if (!loader.dcl || loader.loaded || loader.asyncPending > 0 || loader.queue.length || loader.deferred.length) return;
  loader.loaded = true;
  readyState = 'complete';
  fire(document, 'readystatechange');
  fire(G, 'load');
  fire(G, 'pageshow', Event, {});
}

// Scripts added to the document by script (`appendChild(script)`) run once
// connected: external ones async, inline ones immediately.
function scanInserted(id) {
  const ids = H.kind(id) !== 1 ? [] : H.tag(id) === 'script' ? [id] : H.queryAll(id, 'script');
  for (const sid of ids) {
    if (loader.ran.has(sid) || !isJs(sid)) continue;
    const src = H.getAttr(sid, 'src');
    if (src === null && !H.text(sid).trim()) continue;
    const e = entryFor(sid);
    if (e.src) { e.async = true; loader.asyncPending++; load(e); }
    else execScript(sid, e.text, null);
  }
}

G.__boot = function () {
  loader.started = true;
  for (const id of H.queryAll(0, 'noscript')) H.remove(id);
  bindNamedElements();
  for (const id of H.queryAll(0, 'script')) {
    if (!isJs(id)) { loader.ran.add(id); continue; }
    const e = entryFor(id);
    if (e.src) {
      if (H.getAttr(id, 'async') !== null) { e.async = true; loader.asyncPending++; load(e); continue; }
      if (H.getAttr(id, 'defer') !== null) { loader.deferred.push(e); load(e); continue; }
      load(e);
    }
    loader.queue.push(e);
  }
  pump();
};

// ── Entry points from the browser ──────────────────────────────────────────
G.__tick = function (now) {
  runTimers(now);
  runFrames(now);
};
G.__next = function () {
  if (rafQ.size) return H.now();
  let next = -1;
  for (const t of timerQ.values()) if (next < 0 || t.due < next) next = t.due;
  return next;
};
G.__fetchDone = function (id, status, text, url) {
  const p = pendingFetches.get(id);
  if (!p) return;
  pendingFetches.delete(id);
  p.resolve({ status, text, url });
};
G.__dispatch = function (type, chain, bubbles, cancelable, initJson) {
  const init = initJson ? JSON.parse(initJson) : {};
  init.bubbles = bubbles;
  init.cancelable = cancelable;
  const cls = EVENT_CLASS[type] || Event;
  const ev = new cls(type, init);
  ev.isTrusted = true;
  const path = chain.map(wrap).filter(Boolean);
  if (!path.length) return 0;
  if (path[path.length - 1] !== document) path.push(document);
  path.push(G);
  if (typeof G.onerror !== 'function') {}
  dispatchPath(ev, path);
  return (ev.defaultPrevented ? 1 : 0) | (ev[STOP] & 1 ? 2 : 0);
};
})();
