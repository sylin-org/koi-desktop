// Minimal DOM stub for the workbench's pure-JS layers (node:test).
// The workbench is deliberately framework-free; this harness keeps it that
// way by exercising ui/app.js + ui/sentences.js without a browser. It stubs
// just the surface app.js touches — no HTML parsing, no network, no Tauri.
//
// Usage: node --test ui/

import vm from "node:vm";
import fs from "node:fs";
import path from "node:path";

class ClassList {
  constructor(el) { this.el = el; this.set = new Set(); }
  add(...c) { for (const x of c) this.set.add(x); this._sync(); }
  remove(...c) { for (const x of c) this.set.delete(x); this._sync(); }
  toggle(c, force) {
    const want = force === undefined ? !this.set.has(c) : force;
    if (want) this.set.add(c); else this.set.delete(c);
    this._sync();
  }
  contains(c) { return this.set.has(c); }
  _sync() { this.el._cls = [...this.set].join(" "); }
}

export class El {
  constructor(tag = "div", id = "") {
    this.tagName = String(tag).toUpperCase();
    this.id = id;
    this.children = [];
    this.parentNode = null;
    this.dataset = {};
    this.style = { setProperty: () => {} };
    this._attrs = new Map();
    this._listeners = new Map();
    this._text = "";
    this._html = "";
    this.value = "";
    this.hidden = false;
    this.disabled = false;
    this.checked = false;
    this.title = "";
    this.type = "";
    this._cls = "";
    this.classList = new ClassList(this);
    this.childElementCount = 0;
  }
  get className() { return this._cls; }
  set className(v) {
    this._cls = String(v);
    this.classList.set = new Set(String(v).split(/\s+/).filter(Boolean));
  }
  // Real-DOM semantics: assigning textContent/innerHTML clears the children.
  get textContent() { return this._text; }
  set textContent(v) {
    this._text = String(v);
    for (const c of [...this.children]) c.parentNode = null;
    this.children = [];
    this.childElementCount = 0;
  }
  get innerHTML() { return this._html; }
  set innerHTML(v) {
    this._html = String(v);
    for (const c of [...this.children]) c.parentNode = null;
    this.children = [];
    this.childElementCount = 0;
  }
  setAttribute(k, v) { this._attrs.set(k, String(v)); }
  getAttribute(k) { return this._attrs.has(k) ? this._attrs.get(k) : null; }
  removeAttribute(k) { this._attrs.delete(k); }
  addEventListener(kind, fn) {
    if (!this._listeners.has(kind)) this._listeners.set(kind, []);
    this._listeners.get(kind).push(fn);
  }
  dispatch(kind, event = {}) {
    for (const fn of [...(this._listeners.get(kind) ?? [])]) {
      fn({ stopPropagation() {}, target: this, ...event });
    }
  }
  appendChild(node) {
    if (node.parentNode) node.remove();
    node.parentNode = this;
    this.children.push(node);
    this.childElementCount = this.children.length;
    return node;
  }
  append(...nodes) { for (const n of nodes) this.appendChild(n); }
  remove() {
    if (!this.parentNode) return;
    const i = this.parentNode.children.indexOf(this);
    if (i >= 0) this.parentNode.children.splice(i, 1);
    this.parentNode.childElementCount = this.parentNode.children.length;
    this.parentNode = null;
  }
  replaceChildren(...nodes) {
    for (const c of [...this.children]) c.parentNode = null;
    this.children = [];
    this.append(...nodes);
  }
  querySelector(sel) { return this.querySelectorAll(sel)[0] ?? null; }
  querySelectorAll(sel) {
    const out = [];
    const parts = sel.split(",").map((s) => s.trim());
    const walk = (node) => {
      for (const c of node.children) {
        if (parts.some((s) => matches(c, s))) out.push(c);
        walk(c);
      }
    };
    walk(this);
    return out;
  }
  closest(sel) {
    let node = this.parentNode;
    while (node instanceof El) {
      if (sel.startsWith(".") && node.classList.contains(sel.slice(1))) return node;
      if (sel.startsWith("#") && node.id === sel.slice(1)) return node;
      node = node.parentNode;
    }
    return null;
  }
  getBoundingClientRect() { return { left: 0, top: 0, width: 100, height: 100 }; }
  click() { this.dispatch("click"); }
}

function matches(el, s) {
  if (s.startsWith(".")) return el.classList?.contains(s.slice(1)) ?? false;
  if (s.startsWith("#")) return el.id === s.slice(1);
  return false;
}

// A document whose getElementById lazily creates stub elements, so test IDs
// never drift from the markup the composition root collects.
export function makeDocument() {
  const byId = new Map();
  const document = {
    getElementById(id) {
      if (!byId.has(id)) byId.set(id, new El("div", id));
      return byId.get(id);
    },
    createElement: (tag) => new El(tag),
    createTextNode: (text) => ({ textContent: String(text), nodeType: 3 }),
    querySelector: () => null,
    querySelectorAll: () => [],
    body: new El("body"),
    title: "",
  };
  return { document, byId };
}

export function makeStorage() {
  const map = new Map();
  return {
    getItem: (k) => (map.has(k) ? map.get(k) : null),
    setItem: (k, v) => map.set(k, String(v)),
    removeItem: (k) => map.delete(k),
  };
}

// Load the classic scripts (sentences.js, app.js) into one window-like realm.
// Fetch is a hard reject so a stray network touch fails the test instead of
// silently passing; timers are collected, never fired; the clock is
// controllable so same-millisecond event bursts are testable.
export function loadWorkbench(uiRoot, files = ["sentences.js", "app.js"]) {
  const { document } = makeDocument();
  const timers = [];
  let clock = 1_700_000_000_000;
  const window = {
    addEventListener() {},
    __TAURI__: undefined, // browser mode: read-only paths only
  };
  const globals = {
    window,
    document,
    localStorage: makeStorage(),
    fetch: () => Promise.reject(new Error("no network in tests")),
    setInterval: (fn) => { timers.push(fn); return timers.length; },
    clearInterval: () => {},
    setTimeout: (fn) => { timers.push(fn); return timers.length; },
    clearTimeout: () => {},
    console,
    Date: { now: () => clock },
    __advanceClock: (ms) => { clock += Number(ms); },
  };
  const ctx = vm.createContext(globals);
  for (const f of files) {
    vm.runInContext(fs.readFileSync(path.join(uiRoot, f), "utf8"), ctx, { filename: f });
  }
  return { ctx, document, timers, window };
}

// Run an expression inside the realm (access to lexical bindings like `feed`).
export function probe(ctx, expr) {
  return vm.runInContext(expr, ctx);
}
