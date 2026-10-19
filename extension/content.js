// content.js - the "Download this video" button on players, the download panel, link
// collection for the context menu, and Alt+click to keep a download in the browser.
// Everything is drawn inside one shadow root, so page styles cannot change it and it cannot
// change the page. Text from the page is always set as text, never parsed as HTML.
(() => {
    if (window.__swiftharrydm) return;
    window.__swiftharrydm = true;

    const PRESETS = [
        ['best', 'Best quality'], ['1080', '1080p'], ['720', '720p'], ['480', '480p'],
        ['mp4', 'MP4 (plays everywhere)'], ['mp3', 'MP3 audio'], ['m4a', 'M4A audio'],
    ];
    const MIN_W = 200, MIN_H = 110;

    const send = (msg) => chrome.runtime.sendMessage(msg).catch(() => null);
    const el = (tag, cls, text) => {
        const e = document.createElement(tag);
        if (cls) e.className = cls;
        if (text != null) e.textContent = text;
        return e;
    };

    // ------------------------------------------------------------------ overlay
    let host, root;
    function overlay() {
        if (root) return root;
        host = document.createElement('swiftharrydm-overlay');
        host.style.cssText = 'all: initial; position: fixed; inset: 0 auto auto 0; z-index: 2147483647;';
        root = host.attachShadow({ mode: 'open' });
        const style = el('style');
        style.textContent = `
            :host { all: initial; }
            * { box-sizing: border-box; font: 13px/1.4 "Segoe UI", system-ui, sans-serif; }
            .pill { position: fixed; display: none; align-items: center; gap: 6px; padding: 6px 12px 6px 9px;
                border: 0; border-radius: 18px; background: #2c5f8a; color: #fff; cursor: pointer;
                box-shadow: 0 2px 10px rgba(0,0,0,.35); opacity: .95; }
            .pill:hover { background: #3a7bb3; opacity: 1; }
            .pill svg, .head svg { width: 16px; height: 16px; flex: none; }
            .panel { position: fixed; width: 320px; background: #fff; color: #1c2733; border-radius: 10px;
                box-shadow: 0 10px 30px rgba(0,0,0,.35); overflow: hidden; }
            .head { display: flex; align-items: center; gap: 8px; padding: 10px 12px; background: #2c5f8a; color: #fff; font-weight: 600; }
            .head .x { margin-left: auto; background: none; border: 0; color: #fff; font-size: 18px; cursor: pointer; line-height: 1; }
            .body { padding: 12px; display: flex; flex-direction: column; gap: 10px; }
            .title { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
            select { width: 100%; padding: 6px 8px; border: 1px solid #b7c3d0; border-radius: 5px; background: #fff; color: #1c2733; }
            .row { display: flex; gap: 8px; }
            .btn { flex: 1; padding: 8px; border: 1px solid #b7c3d0; border-radius: 5px; background: #fff; color: #1c2733; cursor: pointer; }
            .btn.primary { background: #2c5f8a; border-color: #2c5f8a; color: #fff; }
            .btn:hover { filter: brightness(1.05); }
            .hint { color: #56657a; font-size: 12px; }
            .toast { position: fixed; right: 16px; bottom: 16px; max-width: 360px; padding: 10px 14px; border-radius: 8px;
                background: #1c2733; color: #fff; box-shadow: 0 6px 20px rgba(0,0,0,.35); }
            .toast.err { background: #a8322b; }
        `;
        root.append(style);
        (document.body || document.documentElement).append(host);
        return root;
    }

    function toast(text, isError) {
        const r = overlay();
        const t = el('div', 'toast' + (isError ? ' err' : ''), text);
        r.append(t);
        setTimeout(() => t.remove(), 4500);
    }

    const ICON = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4v11M7.5 10.5 12 15l4.5-4.5M5 19h14"/></svg>';

    // ------------------------------------------------------------------ media on the page
    function cleanUrl(u) {
        try {
            const url = new URL(u, location.href);
            if (/(^|\.)youtube\.com$/.test(url.hostname) && url.pathname === '/watch') {
                const v = url.searchParams.get('v');
                return v ? `https://www.youtube.com/watch?v=${v}` : url.href;
            }
            return url.href;
        } catch (e) {
            return u;
        }
    }

    /** What to send for a media element: its file if it has a real address, else the page. */
    function describe(media) {
        const src = media.currentSrc || media.src || (media.querySelector('source[src]') || {}).src || '';
        const direct = /^https?:/i.test(src) ? src : null;
        const title = (document.querySelector('meta[property="og:title"]') || {}).content || document.title || 'Video';
        return {
            url: cleanUrl(direct || location.href),
            title: String(title).slice(0, 300),
            type: media.tagName === 'AUDIO' ? 'audio' : 'video',
            duration: isFinite(media.duration) ? media.duration : null,
            pageUrl: location.href,
        };
    }

    const bigEnough = (m) => {
        const r = m.getBoundingClientRect();
        return m.tagName === 'AUDIO' || (r.width >= MIN_W && r.height >= MIN_H);
    };

    // ------------------------------------------------------------------ the button on players
    let current = null;
    let pill = null;
    function place() {
        if (!current || !pill) return;
        const r = current.getBoundingClientRect();
        if (r.bottom < 0 || r.top > innerHeight || r.width === 0) {
            pill.style.display = 'none';
            return;
        }
        pill.style.display = 'flex';
        pill.style.top = `${Math.max(8, r.top + 10)}px`;
        pill.style.left = `${Math.min(innerWidth - 190, r.right - 180)}px`;
    }
    function showPill(media) {
        const r = overlay();
        if (!pill) {
            pill = el('button', 'pill');
            pill.innerHTML = ICON; // fixed markup, no page data
            pill.append(el('span', null, 'Download this video'));
            pill.addEventListener('click', (e) => {
                e.preventDefault();
                e.stopPropagation();
                if (current) openPanel(describe(current), pill.getBoundingClientRect());
            });
            r.append(pill);
        }
        current = media;
        pill.lastChild.textContent = media.tagName === 'AUDIO' ? 'Download this audio' : 'Download this video';
        place();
    }
    document.addEventListener('mouseover', (e) => {
        const m = e.target instanceof Element ? e.target.closest('video, audio') : null;
        if (m && bigEnough(m)) showPill(m);
    }, true);
    addEventListener('scroll', place, true);
    addEventListener('resize', place);

    // ------------------------------------------------------------------ the download panel
    let panel = null;
    function closePanel() {
        if (panel) panel.remove();
        panel = null;
    }
    function openPanel(item, near) {
        closePanel();
        const r = overlay();
        panel = el('div', 'panel');
        const head = el('div', 'head');
        head.innerHTML = ICON;
        head.append(el('span', null, 'SwiftHarryDM'));
        const x = el('button', 'x', '×');
        x.title = 'Close';
        x.addEventListener('click', closePanel);
        head.append(x);
        const body = el('div', 'body');
        body.append(el('div', 'title', item.title));
        const select = el('select', 'format-select');
        const presets = el('optgroup');
        presets.label = 'Quality';
        for (const [v, l] of PRESETS) {
            const o = el('option', null, l);
            o.value = v;
            presets.append(o);
        }
        select.append(presets);
        body.append(select);
        const hint = el('div', 'hint', 'Loading the formats from SwiftHarryDM…');
        body.append(hint);
        const streams = [];
        const row = el('div', 'row');
        const now = el('button', 'btn primary download-now-btn', 'Download');
        const later = el('button', 'btn', 'Download later');
        now.addEventListener('click', () => submit(item, select.value, true, streams));
        later.addEventListener('click', () => submit(item, select.value, false, streams));
        row.append(now, later);
        body.append(row);
        panel.append(head, body);
        const top = near ? Math.min(innerHeight - 260, near.bottom + 8) : innerHeight / 2 - 120;
        const left = near ? Math.max(8, Math.min(innerWidth - 330, near.right - 320)) : innerWidth / 2 - 160;
        panel.style.top = `${Math.max(8, top)}px`;
        panel.style.left = `${left}px`;
        r.append(panel);
        fill(select, hint, item.url, streams);
    }

    const STREAM_LABEL = { hls: 'HLS stream', dash: 'DASH stream', file: 'Media file' };
    const size = (n) => (n ? (n >= 1 << 30 ? `${(n / (1 << 30)).toFixed(2)} GB` : `${Math.round(n / (1 << 20))} MB`) : '');

    /** Exact formats from the app, plus the streams this page's player loaded. */
    async function fill(select, hint, url, streams) {
        const [formatsOk, s] = await Promise.all([fillFormats(select, hint, url), send({ action: 'getStreams' })]);
        const list = s && s.success ? s.streams : [];
        if (!list.length) return;
        streams.push(...list);
        const group = el('optgroup');
        group.label = 'Streams on this page';
        list.forEach((st, i) => {
            let name = '';
            try {
                name = decodeURIComponent(new URL(st.url).pathname.split('/').pop() || '');
            } catch (e) {
                name = '';
            }
            const o = el('option', null, [STREAM_LABEL[st.kind] || 'Stream', name, st.kind === 'file' ? size(st.size) : ''].filter(Boolean).join(' \u00b7 '));
            o.value = `s:${i}`;
            group.append(o);
        });
        select.append(group);
        if (!formatsOk) {
            select.value = 's:0';
            hint.textContent = 'This site is not known to the video downloader; the stream the player loaded is selected.';
        }
    }

    async function fillFormats(select, hint, url) {
        const r = await send({ action: 'mediaInfo', url });
        const formats = r && r.success && r.info ? r.info.formats : [];
        if (!formats.length) {
            hint.textContent = r && r.error ? `Formats unavailable: ${r.error}` : 'Choose a quality; SwiftHarryDM picks the matching format.';
            return false;
        }
        hint.textContent = formats.length === 1 ? '1 format available.' : `${formats.length} formats available.`;
        const group = el('optgroup');
        group.label = 'Exact formats';
        for (const f of formats.slice(0, 60)) {
            const kind = f.vcodec && !f.acodec ? 'video only, best audio added' : !f.vcodec && f.acodec ? 'audio' : '';
            const o = el('option', null, [f.height ? `${f.height}p` : 'audio', f.ext, kind, size(f.size), f.note].filter(Boolean).join(' · '));
            o.value = `f:${f.id}`;
            group.append(o);
        }
        select.append(group);
        return true;
    }

    async function submit(item, choice, instant, streams) {
        closePanel();
        let data = { ...item, format: 'best', instantDownload: instant };
        if (choice.startsWith('s:')) {
            // A stream the player loaded: playlists go to the video downloader, files to the
            // fast segmented engine.
            const st = streams[Number(choice.slice(2))];
            data = { url: st.url, title: item.title, pageUrl: location.href, format: 'best', instantDownload: instant, type: st.kind === 'file' ? null : 'video' };
        } else if (choice.startsWith('f:')) data.formatId = choice.slice(2);
        else data.format = choice;
        const r = await send({ action: 'downloadMedia', data });
        if (r && r.success) toast(`Sent to SwiftHarryDM: ${r.message || ''}`);
        else toast((r && r.error) || 'SwiftHarryDM is not running.', true);
    }

    // ------------------------------------------------------------------ commands from the extension
    function links(selectionOnly) {
        let anchors = [...document.querySelectorAll('a[href]')];
        if (selectionOnly) {
            const sel = getSelection();
            anchors = anchors.filter((a) => sel && sel.rangeCount && sel.containsNode(a, true));
        }
        return anchors.map((a) => a.href).filter((h) => /^https?:/i.test(h));
    }

    function allMedia() {
        const items = [...document.querySelectorAll('video, audio')].filter(bigEnough).map(describe);
        const seen = new Set();
        return items.filter((i) => !seen.has(i.url) && seen.add(i.url));
    }

    chrome.runtime.onMessage.addListener((req) => {
        switch (req.action) {
            case 'collectLinks':
                send({ action: 'sendLinks', urls: links(!!req.selectionOnly), pageUrl: location.href });
                break;
            case 'contextMenuDownload': {
                const c = req.context || {};
                const url = c.srcUrl || c.linkUrl || c.pageUrl || location.href;
                const type = c.mediaType === 'audio' ? 'audio' : c.mediaType === 'video' ? 'video' : null;
                if (type && !/^https?:/i.test(c.srcUrl || '')) {
                    openPanel({ url: cleanUrl(location.href), title: document.title, type }, null);
                } else {
                    openPanel({ url: cleanUrl(url), title: c.linkText || document.title, type: type || 'video' }, null);
                }
                break;
            }
            case 'downloadPageMedia':
            case 'showMediaDetection': {
                const items = allMedia();
                if (items.length) {
                    openPanel(items[0], null);
                    break;
                }
                // Players inside other frames, or audio without an element: offer the streams seen.
                send({ action: 'getStreams' }).then((r) => {
                    if (r && r.success && r.streams.length) openPanel({ url: cleanUrl(location.href), title: document.title || 'Video', type: 'video', pageUrl: location.href }, null);
                    else toast('No video or audio found on this page.', true);
                });
                break;
            }
        }
    });

    // Alt+click on a link leaves that download to the browser.
    document.addEventListener('mousedown', (e) => {
        if (!e.altKey) return;
        const a = e.target instanceof Element ? e.target.closest('a[href]') : null;
        if (a && /^https?:/i.test(a.href)) send({ action: 'bypassNext', url: a.href });
    }, true);
})();
