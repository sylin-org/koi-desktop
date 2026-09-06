/* Observe authoritative connection count/readiness; never retain invitations. */
(() => {
  const panel = document.getElementById('browser-access');
  if (!panel) return;
  let stopped = false, dirty = false, timer;
  panel.querySelector('form[action="/web/settings"]')?.addEventListener('change', () => { dirty = true; });
  const check = async () => {
    try {
      const response = await fetch('/web/status', { cache: 'no-store', signal: AbortSignal.timeout(5000) });
      if (!stopped && response.ok) {
        const status = await response.json();
        if (stopped) return;
        if (!dirty && (String(status.sessions.length) !== panel.dataset.sessions || String(status.phone_ready) !== panel.dataset.ready)) {
          location.replace('/web'); return;
        }
      }
    } catch (_) { /* Keep the current page; the next observation may recover. */ }
    if (!stopped) timer = setTimeout(check, 2000);
  };
  timer = setTimeout(check, 2000);
  window.addEventListener('pagehide', () => { stopped = true; clearTimeout(timer); });
  window.addEventListener('pageshow', event => { if (event.persisted) { stopped = false; clearTimeout(timer); timer = setTimeout(check, 2000); } });
})();
