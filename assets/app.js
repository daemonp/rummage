// Rummage App — client-side interactivity for the Instrument UI.
// Server renders full pages; JS enhances with fast navigation and keyboard shortcuts.
(function() {
  'use strict';

  // ── State ───────────────────────────────────────────────────────
  let selectedThreadId = null;
  let threadIds = [];
  let selectedIndex = -1;
  let paletteOpen = false;
  let paletteQuery = '';
  let paletteActive = 0;
  let paletteItems = [];

  // ── DOM ready ───────────────────────────────────────────────────
  function init() {
    discoverThreads();
    bindKeyboard();
    bindThreadClicks();
    bindMessageCollapsing();
    bindSearchForm();
    bindThemeToggle();
    bindExpandAll();
    bindSearchTriggerKbd();
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }

  // ── Thread discovery ────────────────────────────────────────────
  function discoverThreads() {
    const rows = document.querySelectorAll('.i-row[data-thread-id]');
    threadIds = Array.from(rows).map(r => r.dataset.threadId);
    const selected = document.querySelector('.i-row.selected');
    if (selected) {
      selectedThreadId = selected.dataset.threadId;
      selectedIndex = threadIds.indexOf(selectedThreadId);
    }
  }

  // ── Keyboard shortcuts ──────────────────────────────────────────
  function bindKeyboard() {
    document.addEventListener('keydown', (e) => {
      const tag = document.activeElement?.tagName;

      // Palette open: palette-specific keys
      if (paletteOpen) {
        if (e.key === 'Escape') { e.preventDefault(); closePalette(); return; }
        if (e.key === 'ArrowDown') { e.preventDefault(); paletteActive = Math.min(flatPaletteCount() - 1, paletteActive + 1); updatePaletteList(); return; }
        if (e.key === 'ArrowUp') { e.preventDefault(); paletteActive = Math.max(0, paletteActive - 1); updatePaletteList(); return; }
        if (e.key === 'Enter') { e.preventDefault(); executePaletteAction(); return; }
        return;
      }

      // ⌘K or Ctrl+K — command palette (must be before input guard)
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        openCommandPalette();
        return;
      }

      // Ignore if typing in an input
      if (tag === 'INPUT' || tag === 'TEXTAREA') return;

      // / — focus search
      if (e.key === '/' && !e.metaKey && !e.ctrlKey) {
        e.preventDefault();
        focusSearch();
        return;
      }

      // ? — help
      if (e.key === '?' && !e.metaKey && !e.ctrlKey) {
        e.preventDefault();
        window.location.href = '/';
        return;
      }

      // u — raw .eml
      if (e.key === 'u' && !e.metaKey && !e.ctrlKey) {
        e.preventDefault();
        openRawEml();
        return;
      }

      // j / k — next / prev thread
      if (e.key === 'j' || e.key === 'k') {
        e.preventDefault();
        const delta = e.key === 'j' ? 1 : -1;
        navigateThread(delta);
        return;
      }

      // o or Enter — open selected thread
      if ((e.key === 'o' || e.key === 'Enter') && !e.metaKey && !e.ctrlKey) {
        e.preventDefault();
        openSelectedThread();
        return;
      }
    });
  }

  function focusSearch() {
    const input = document.querySelector('.i-search-form input');
    if (input) {
      input.focus();
      input.select();
    }
  }

  function openRawEml() {
    const selected = document.querySelector('.i-row.selected');
    if (!selected) return;
    const firstMsg = selected.querySelector('[data-message-id]');
    if (!firstMsg) return;
    const msgId = firstMsg.dataset.messageId;
    if (msgId) {
      window.open('/api/message/' + encodeURIComponent(msgId), '_blank');
    }
  }

  function navigateThread(delta) {
    if (threadIds.length === 0) return;
    selectedIndex = Math.max(0, Math.min(threadIds.length - 1, selectedIndex + delta));
    const id = threadIds[selectedIndex];
    selectThread(id, true);
  }

  function openSelectedThread() {
    const selected = document.querySelector('.i-row.selected');
    if (!selected) return;
    const id = selected.dataset.threadId;
    if (id) {
      window.location.href = '/thread/' + encodeURIComponent(id);
    }
  }

  // ── Thread selection via click ──────────────────────────────────
  function bindThreadClicks() {
    document.querySelectorAll('.i-row[data-thread-id]').forEach(row => {
      row.addEventListener('click', (e) => {
        if (e.target.closest('a')) return;
        const id = row.dataset.threadId;
        if (id) selectThread(id, true);
      });
    });
  }

  function selectThread(id, fetchDetail) {
    document.querySelectorAll('.i-row').forEach(r => r.classList.remove('selected'));
    const row = document.querySelector('.i-row[data-thread-id="' + id + '"]');
    if (row) {
      row.classList.add('selected');
      row.scrollIntoView({ block: 'nearest' });
      selectedThreadId = id;
      selectedIndex = threadIds.indexOf(id);
    }
    if (fetchDetail) fetchThreadDetail(id);
  }

  // ── Fetch thread detail via API ─────────────────────────────────
  function fetchThreadDetail(id) {
    const reader = document.querySelector('.i-reader');
    if (!reader) return;
    reader.classList.add('loading');

    fetch('/api/thread/' + encodeURIComponent(id))
      .then(r => {
        if (!r.ok) throw new Error('HTTP ' + r.status);
        return r.json();
      })
      .then(data => {
        renderThreadDetail(reader, data);
        reader.classList.remove('loading');
        if (window.history.replaceState) {
          window.history.replaceState({ threadId: id }, '', '/search?q=' + encodeURIComponent(getQuery()) + '&thread=' + encodeURIComponent(id));
        }
      })
      .catch(err => {
        reader.classList.remove('loading');
        reader.innerHTML = '<div class="i-placeholder mono"><div class="i-placeholder-line">// error</div><div class="i-placeholder-text">' + escapeHtml(err.message) + '</div></div>';
      });
  }

  function getQuery() {
    return new URLSearchParams(window.location.search).get('q') || '';
  }

  function renderThreadDetail(container, detail) {
    const msgCount = detail.messages.length;
    const subject = escapeHtml(detail.messages[0]?.subject || '');

    // Compute participants
    const participants = new Set(detail.messages.map(m => m.headers.from));
    const participantCount = participants.size;

    // Compute date range
    const dates = detail.messages.map(m => m.date).filter(d => d > 0);
    const oldest = dates.length ? Math.min(...dates) : 0;
    const newest = dates.length ? Math.max(...dates) : 0;

    let html = '';
    html += '<div class="i-reader-head">';
    html += '<div class="i-reader-eyebrow mono"><span>thread</span><span class="i-reader-id">' + escapeHtml(detail.thread_id) + '</span></div>';
    html += '<h1 class="i-reader-subj">' + subject + '</h1>';
    html += '<div class="i-reader-meta mono">';
    html += '<span><b class="tnum">' + msgCount + '</b> ' + (msgCount === 1 ? 'message' : 'messages') + '</span>';
    html += '<span class="i-sep-light">·</span>';
    html += '<span><b class="tnum">' + participantCount + '</b> ' + (participantCount === 1 ? 'participant' : 'participants') + '</span>';
    if (oldest && newest) {
      html += '<span class="i-sep-light">·</span>';
      html += '<span>' + fmtDate(oldest, 'short') + ' → ' + fmtDate(newest, 'short') + '</span>';
    }
    html += '</div>';
    html += '<div class="i-reader-actions">';
    html += '<a class="i-btn" href="/api/message/' + encodeURIComponent(detail.messages[0]?.message_id || detail.thread_id) + '">';
    html += '<svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M8 2v9M4.5 7.5L8 11l3.5-3.5M3 13h10"/></svg>';
    html += ' raw .eml</a>';
    html += '<button class="i-btn" data-action="expand-all">';
    html += '<svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M4 6l4 4 4-4"/></svg>';
    html += ' expand all</button>';
    html += '</div>';
    html += '</div>';

    detail.messages.forEach((m, i) => {
      const collapsed = i > 0 ? ' collapsed' : '';
      html += '<article class="i-msg' + collapsed + '" data-message-id="' + escapeHtml(m.message_id) + '">';
      html += '<header class="i-msg-head">';
      html += '<div class="i-msg-head-left">';
      html += '<span class="i-msg-num mono tnum">' + String(i + 1).padStart(2, '0') + '<span class="i-msg-num-sep">/</span>' + String(msgCount).padStart(2, '0') + '</span>';
      html += '<div class="i-msg-author">';
      html += '<div class="i-msg-author-name">' + escapeHtml(parseName(m.headers.from)) + '</div>';
      const email = parseEmail(m.headers.from);
      if (email) html += '<div class="i-msg-author-email mono">&lt;' + escapeHtml(email) + '&gt;</div>';
      html += '</div></div>';
      html += '<div class="i-msg-head-right mono">';
      html += '<span class="i-msg-to">→ ' + escapeHtml(m.headers.to) + '</span>';
      html += '<span class="i-msg-date">' + fmtDate(m.date, 'iso') + '</span>';
      html += '<span class="i-msg-collapse">';
      html += '<svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M4 6l4 4 4-4"/></svg>';
      html += '</span></div></header>';

      html += '<div class="i-msg-body">';
      if (m.content) html += '<div class="prose">' + m.content + '</div>';
      if (m.attachments && m.attachments.length > 0) {
        html += '<div class="i-msg-atts">';
        html += '<div class="i-msg-atts-label mono">';
        html += '<svg width="11" height="11" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M11.5 7L6 12.5a2.5 2.5 0 1 1-3.5-3.5L8 3.5a1.5 1.5 0 1 1 2 2L4.5 11"/></svg>';
        html += ' ' + m.attachments.length + ' attachment' + (m.attachments.length !== 1 ? 's' : '') + '</div>';
        m.attachments.forEach(a => {
          const attUrl = '/api/attachment?msg=' + encodeURIComponent(m.message_id) + '&part=' + a.part;
          const fname = escapeHtml(a.filename || ('part-' + a.part));
          const ctype = escapeHtml(a.content_type);
          const isImage = (a.content_type || '').startsWith('image/');
          const isVideo = (a.content_type || '').startsWith('video/');
          const isPdf = a.content_type === 'application/pdf' || (a.filename || '').endsWith('.pdf');
          const isText = a.content_type === 'text/plain' || a.content_type === 'text/log' || (a.filename || '').endsWith('.log') || (a.filename || '').endsWith('.txt');
          const iconPath = isImage ? ICON_PATHS.image || ICON_PATHS.dot : ICON_PATHS.file || ICON_PATHS.dot;
          const iconSvg = '<svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="' + iconPath + '"/></svg>';
          const dlSvg = '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M8 2v9M4.5 7.5L8 11l3.5-3.5M3 13h10"/></svg>';

          html += '<div class="i-msg-att"><div class="i-att-chrome">';
          html += '<div class="i-att-chrome-bar">';
          html += '<a class="i-att-chrome-info" href="' + attUrl + '" target="_blank" title="Open ' + fname + '">' + iconSvg + ' ' + fname + ' · ' + ctype + '</a>';
          html += '<span class="i-att-chrome-actions"><a href="' + attUrl + '" target="_blank" title="Download" class="i-att-chrome-dl">' + dlSvg + '</a></span>';
          html += '</div>';
          if (isPdf) {
            html += '<iframe src="' + attUrl + '" class="i-att-pdf-frame" title="PDF preview: ' + fname + '"></iframe>';
          } else if (isImage) {
            html += '<div class="i-att-img-wrap"><img src="' + attUrl + '" alt="' + fname + '" class="i-att-img" /></div>';
          } else if (isVideo) {
            html += '<div class="i-att-video-wrap"><video src="' + attUrl + '" class="i-att-video" controls preload="metadata"></video></div>';
          } else if (isText) {
            html += '<iframe src="' + attUrl + '" class="i-att-log-frame" title="Text preview: ' + fname + '"></iframe>';
          }
          html += '</div></div>';
        });
        html += '</div>';
      }
      html += '</div></article>';
    });

    container.innerHTML = html;
    bindMessageCollapsing();
    bindExpandAll();
  }

  function parseName(from) {
    const m = from.match(/^([^<]+)</);
    return m ? m[1].trim() : from;
  }

  function parseEmail(from) {
    const m = from.match(/<([^>]+)>/);
    return m ? m[1] : '';
  }

  // ── Message collapsing ──────────────────────────────────────────
  function bindMessageCollapsing() {
    document.querySelectorAll('.i-msg-head').forEach(head => {
      head.addEventListener('click', (e) => {
        // Don't collapse if clicking a link or button inside the header
        if (e.target.closest('a') || e.target.closest('button')) return;
        const msg = head.closest('.i-msg');
        if (msg) msg.classList.toggle('collapsed');
      });
    });
  }

  function bindExpandAll() {
    document.querySelectorAll('[data-action="expand-all"]').forEach(btn => {
      btn.addEventListener('click', () => {
        document.querySelectorAll('.i-msg.collapsed').forEach(msg => msg.classList.remove('collapsed'));
      });
    });
  }

  // ── Search form ─────────────────────────────────────────────────
  function bindSearchForm() {
    const form = document.querySelector('.i-search-form');
    if (!form) return;
    // Let the form submit normally — server handles it
  }

  function bindSearchTriggerKbd() {
    const el = document.querySelector('.i-search-trigger-kbd');
    if (el) {
      el.style.cursor = 'pointer';
      el.addEventListener('click', (e) => {
        e.preventDefault();
        e.stopPropagation();
        openCommandPalette();
      });
    }
  }

  // ── Command palette ─────────────────────────────────────────────
  function openCommandPalette() {
    paletteOpen = true;
    paletteQuery = '';
    paletteActive = 0;
    buildPaletteItems();
    renderPaletteOverlay();
  }

  function closePalette() {
    paletteOpen = false;
    const el = document.querySelector('.palette-backdrop');
    if (el) el.remove();
  }

  function buildPaletteItems() {
    const baseActions = [
      { section: 'Search', items: [
        { id: 'q-recent-1', label: 'tag:inbox', hint: 'recent', kind: 'query' },
        { id: 'q-recent-2', label: 'has:attachment', hint: 'recent', kind: 'query' },
        { id: 'q-recent-3', label: 'from:alice@example.com', hint: 'recent', kind: 'query' },
      ]},
      { section: 'Operators', items: [
        { id: 'op-from', label: 'from:', hint: 'sender', kind: 'operator' },
        { id: 'op-to', label: 'to:', hint: 'recipient', kind: 'operator' },
        { id: 'op-tag', label: 'tag:', hint: 'label', kind: 'operator' },
        { id: 'op-has', label: 'has:attachment', hint: 'filter', kind: 'operator' },
        { id: 'op-date', label: 'date:YYYY-MM..YYYY-MM', hint: 'range', kind: 'operator' },
        { id: 'op-bool', label: 'AND / OR / NOT', hint: 'boolean', kind: 'operator' },
      ]},
      { section: 'Jump to', items: [
        { id: 'jump-inbox', label: 'tag:inbox', hint: 'inbox', kind: 'tag' },
        { id: 'jump-unread', label: 'tag:unread', hint: 'unread', kind: 'tag' },
      ]},
      { section: 'Actions', items: [
        { id: 'act-theme', label: 'Toggle theme', hint: '⇧⌘L', kind: 'action' },
        { id: 'act-help', label: 'Help page', hint: '?', kind: 'action' },
      ]},
    ];

    const q = paletteQuery.toLowerCase().trim();
    if (!q) {
      paletteItems = baseActions;
      return;
    }

    paletteItems = baseActions.map(s => ({
      section: s.section,
      items: s.items.filter(i =>
        i.label.toLowerCase().includes(q) || (i.hint || '').toLowerCase().includes(q)
      ),
    })).filter(s => s.items.length > 0);
  }

  function flatPaletteCount() {
    let n = 0;
    for (const sec of paletteItems) n += sec.items.length;
    return n;
  }

  function updatePaletteList() {
    const container = document.querySelector('.palette-list');
    if (container) renderPaletteList(container);
  }

  function renderPaletteOverlay() {
    // Remove existing
    const existing = document.querySelector('.palette-backdrop');
    if (existing) existing.remove();

    const backdrop = document.createElement('div');
    backdrop.className = 'palette-backdrop';
    backdrop.addEventListener('click', closePalette);

    const palette = document.createElement('div');
    palette.className = 'palette';
    palette.addEventListener('click', e => e.stopPropagation());

    // Input
    const input = document.createElement('input');
    input.className = 'palette-input mono';
    input.placeholder = 'Search the archive — try from:kalle has:attachment';
    input.value = paletteQuery;
    input.addEventListener('input', (e) => {
      paletteQuery = e.target.value;
      paletteActive = 0;
      buildPaletteItems();
      renderPaletteList(listContainer);
    });
    palette.appendChild(input);

    // List
    const listContainer = document.createElement('div');
    listContainer.className = 'palette-list scroll';
    renderPaletteList(listContainer);
    palette.appendChild(listContainer);

    // Footer
    const foot = document.createElement('div');
    foot.className = 'palette-foot';
    const modKey = /Mac|iPhone|iPad/.test(navigator.platform) ? '⌘' : 'Ctrl+';
    foot.innerHTML = '<div class="group"><span><span class="kbd">↑</span> <span class="kbd">↓</span> navigate</span><span><span class="kbd">↵</span> select</span><span><span class="kbd">esc</span> close</span><span><span class="kbd">' + modKey + 'K</span> toggle</span></div>';
    palette.appendChild(foot);

    backdrop.appendChild(palette);
    document.body.appendChild(backdrop);

    // Focus input after a brief delay to let the DOM settle
    setTimeout(() => input.focus(), 10);
  }

  function renderPaletteList(container) {
    container.innerHTML = '';

    if (paletteItems.length === 0) {
      container.innerHTML = '<div style="padding:24px 20px;color:var(--fg-4);font-family:var(--font-mono);font-size:12px">No matches. Press Enter to search for "' + escapeHtml(paletteQuery) + '".</div>';
      return;
    }

    let flatIdx = 0;
    paletteItems.forEach(sec => {
      const secLabel = document.createElement('div');
      secLabel.className = 'palette-section-label';
      secLabel.textContent = sec.section;
      container.appendChild(secLabel);

      sec.items.forEach(item => {
        const idx = flatIdx; // capture by value for closures
        const row = document.createElement('div');
        row.className = 'palette-item' + (idx === paletteActive ? ' active' : '');
        row.addEventListener('mouseenter', () => { paletteActive = idx; renderPaletteList(container); });
        row.addEventListener('click', () => {
          paletteActive = idx;
          executePaletteAction();
        });

        const iconName = item.kind === 'query' ? 'search' : item.kind === 'tag' ? 'tag' : item.kind === 'operator' ? 'filter' : 'sliders';
        const iconSvg = '<svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="' + (ICON_PATHS[iconName] || ICON_PATHS.dot) + '"/></svg>';

        row.innerHTML = '<span class="icon">' + iconSvg + '</span><span class="label mono">' + escapeHtml(item.label) + '</span><span class="hint">' + escapeHtml(item.hint || '') + '</span>';
        container.appendChild(row);
        flatIdx++;
      });
    });
  }

  function executePaletteAction() {
    let flatIdx = 0;
    let item = null;
    for (const sec of paletteItems) {
      for (const it of sec.items) {
        if (flatIdx === paletteActive) { item = it; break; }
        flatIdx++;
      }
      if (item) break;
    }

    if (!item && paletteQuery.trim()) {
      item = { kind: 'query', label: paletteQuery };
    }

    if (!item) { closePalette(); return; }

    if (item.kind === 'query' || item.kind === 'tag') {
      window.location.href = '/search?q=' + encodeURIComponent(item.label);
    } else if (item.kind === 'operator') {
      const input = document.querySelector('.i-search-form input');
      if (input) {
        const current = input.value;
        const needsSpace = current.length > 0 && !current.endsWith(' ');
        input.value = current + (needsSpace ? ' ' : '') + item.label;
        input.focus();
      }
      closePalette();
      return;
    } else if (item.kind === 'action') {
      if (item.id === 'act-theme') {
        const btn = document.querySelector('.i-theme-toggle');
        if (btn) btn.click();
      } else if (item.id === 'act-help') {
        window.location.href = '/';
      }
    }

    closePalette();
  }

  const ICON_PATHS = {
    search: 'M14 14l-3.5-3.5M11.5 6.5a5 5 0 1 1-10 0 5 5 0 0 1 10 0z',
    tag: 'M2 2h6l6 6-6 6-6-6z M5 5h.01',
    filter: 'M1 3h14l-5.5 6.5V14L6.5 12V9.5z',
    sliders: 'M2 4h6 M10 4h4 M2 8h2 M6 8h8 M2 12h10 M12 12h2 M9 4a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3z M5 8a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3z M11 12a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3z',
    dot: 'M8 8m-2 0a2 2 0 1 0 4 0 2 2 0 1 0 -4 0',
    image: 'M2 3h12v10H2z M5.5 6.5a1 1 0 1 1-2 0 1 1 0 0 1 2 0 M2 11l3-3 3 3 2-2 4 4',
    file: 'M3 2h6l4 4v8H3z M9 2v4h4',
    download: 'M8 2v9M4.5 7.5L8 11l3.5-3.5M3 13h10',
  };

  // ── Theme toggle ────────────────────────────────────────────────
  function bindThemeToggle() {
    const btn = document.querySelector('.i-theme-toggle');
    if (!btn) return;
    btn.addEventListener('click', () => {
      const body = document.body;
      const isLight = body.classList.contains('light');
      if (isLight) {
        body.classList.remove('light');
        body.classList.add('dark');
        document.documentElement.dataset.theme = 'dark';
        setCookie('theme', 'dark', 365);
      } else {
        body.classList.remove('dark');
        body.classList.add('light');
        document.documentElement.dataset.theme = 'light';
        setCookie('theme', 'light', 365);
      }
    });
  }

  function setCookie(name, value, days) {
    const expires = new Date(Date.now() + days * 864e5).toUTCString();
    document.cookie = name + '=' + encodeURIComponent(value) + '; expires=' + expires + '; path=/';
  }

  // ── Helpers ─────────────────────────────────────────────────────
  function escapeHtml(text) {
    if (text == null) return '';
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
  }

  function fmtDate(ts, mode) {
    const d = new Date(ts * 1000);
    const now = new Date();
    const diff = (now - d) / 1000;
    if (mode === 'iso') {
      return d.toISOString().slice(0, 16).replace('T', ' ');
    }
    if (mode === 'short') {
      return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: '2-digit' });
    }
    if (diff < 3600) return Math.floor(diff / 60) + 'm';
    if (diff < 86400) return Math.floor(diff / 3600) + 'h';
    if (diff < 86400 * 7) return Math.floor(diff / 86400) + 'd';
    return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
  }
})();
