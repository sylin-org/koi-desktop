import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
const source = readFileSync(new URL('./browser-access-status.js', import.meta.url), 'utf8');
function harness(fetcher = async () => ({ ok: true, json: async () => ({sessions: [], phone_ready: false}) })) {
  const timers = new Map(), events = {}, edits = {}, visits = [];
  let id = 0;
  const context = {
    document: { getElementById: () => ({ dataset: {sessions: '0', ready: 'false'}, querySelector: () => ({ addEventListener: (type, cb) => { edits[type] = cb; } }) }) },
    window: { addEventListener: (type, cb) => { events[type] = cb; } },
    location: { replace: path => visits.push(path) },
    fetch: fetcher, AbortSignal,
    setTimeout: cb => { timers.set(++id, cb); return id; },
    clearTimeout: id => timers.delete(id),
  };
  vm.runInNewContext(source, context);
  return { events, edits, visits, timers, tick: async () => { const [id, cb] = timers.entries().next().value; timers.delete(id); await cb(); } };
}
const connected = async () => ({ ok: true, json: async () => ({sessions: [{}], phone_ready: true}) });
test('native browser access returns to authoritative connections after pairing', async () => {
  const h = harness(connected); await h.tick();
  assert.deepEqual(h.visits, ['/web']); assert.equal(h.timers.size, 0);
});
test('native status preserves unsaved settings and continues observation', async () => {
  const h = harness(connected); h.edits.change(); await h.tick();
  assert.deepEqual(h.visits, []); assert.equal(h.timers.size, 1);
});
test('leaving prevents late status navigation; restored pages resume observation', async () => {
  let resolve;
  const h = harness(() => new Promise(r => { resolve = r; }));
  const pending = h.tick(); h.events.pagehide(); resolve(await connected()); await pending;
  assert.deepEqual(h.visits, []); assert.equal(h.timers.size, 0);
  h.events.pageshow({persisted: true}); assert.equal(h.timers.size, 1);
});
test('failed native observation retains the current page and retries', async () => {
  const h = harness(async () => { throw new Error('Daemon restarting'); }); await h.tick();
  assert.deepEqual(h.visits, []); assert.equal(h.timers.size, 1);
});
