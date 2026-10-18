// background.js - talks to the SwiftHarryDM desktop app on 127.0.0.1.
// The app only accepts requests carrying a token obtained by pairing: the app shows
// a one-time code, the user types it into this extension's popup.
// API 3 adds taking over browser downloads (the app decides which ones by file type,
// size and exception lists) and sending all or selected links of a page.
const PORTS = [5001, 5002];
// Hold Alt while clicking a link to leave that download to the browser.
const BYPASS_MS = 4000;

class BackgroundManager {
    constructor() {
        this.appPort = null;
        this.apiVersion = 0;
        this.bypass = new Map(); // url -> time the user Alt-clicked it
        this.resent = new Set(); // urls handed back to the browser after a failed take-over
        this.seen = new Set(); // download ids already considered (two events fire per download)
        this.createContextMenu();
        this.setupMessageListener();
        this.setupCapture();
    }

    async token() {
        const { swiftdmToken } = await chrome.storage.local.get('swiftdmToken');
        return swiftdmToken || null;
    }

    async request(path, { method = 'GET', body, auth = true, timeoutMs = 5000 } = {}) {
        const port = await this.findAppPort();
        if (!port) {
            return { ok: false, status: 0, data: { error: 'SwiftHarryDM is not running' } };
        }
        const headers = {};
        if (body !== undefined) headers['Content-Type'] = 'application/json';
        if (auth) {
            const t = await this.token();
            if (t) headers['Authorization'] = `Bearer ${t}`;
        }
        const ctrl = new AbortController();
        const timer = setTimeout(() => ctrl.abort(), timeoutMs);
        try {
            const r = await fetch(`http://127.0.0.1:${port}${path}`, {
                method, headers, body: body === undefined ? undefined : JSON.stringify(body), signal: ctrl.signal
            });
            let data = {};
            try { data = await r.json(); } catch (e) { /* non-JSON error */ }
            return { ok: r.ok, status: r.status, data };
        } catch (e) {
            this.appPort = null;
            return { ok: false, status: 0, data: { error: 'Cannot reach SwiftHarryDM' } };
        } finally {
            clearTimeout(timer);
        }
    }

    async findAppPort() {
        if (this.appPort) return this.appPort;
        for (const port of PORTS) {
            const ctrl = new AbortController();
            const timer = setTimeout(() => ctrl.abort(), 1500);
            try {
                const r = await fetch(`http://127.0.0.1:${port}/health`, { signal: ctrl.signal });
                const data = await r.json();
                if (r.ok && data.app === 'SwiftHarryDM') {
                    this.appPort = port;
                    this.apiVersion = data.api || 0;
                    return port;
                }
            } catch (e) {
                // try the next port
            } finally {
                clearTimeout(timer);
            }
        }
        return null;
    }

    async status() {
        const r = await this.request('/health');
        if (r.status === 0) return { connected: false, paired: false, error: r.data.error };
        return { connected: r.ok, paired: !!r.data.paired, version: r.data.version, api: r.data.api };
    }

    async pairStart() {
        const r = await this.request('/api/pair', { method: 'POST', auth: false });
        if (r.status === 202) return { success: true, pairingId: r.data.pairing_id };
        return { success: false, error: r.data.error || `HTTP ${r.status}` };
    }

    async pairConfirm(pairingId, code) {
        const r = await this.request('/api/pair/confirm', {
            method: 'POST', auth: false, body: { pairing_id: pairingId, code: String(code || '').trim() }
        });
        if (r.ok && r.data.token) {
            await chrome.storage.local.set({ swiftdmToken: r.data.token });
            return { success: true };
        }
        return { success: false, error: r.data.error || `HTTP ${r.status}`, status: r.status };
    }

    async unpair() {
        await chrome.storage.local.remove('swiftdmToken');
        return { success: true };
    }

    // ------------------------------------------------------------ taking over downloads
    async captureEnabled() {
        const { swiftdmCapture } = await chrome.storage.local.get('swiftdmCapture');
        return swiftdmCapture !== false;
    }

    async cookiesFor(url) {
        try {
            const ok = await chrome.permissions.contains({ permissions: ['cookies'], origins: ['<all_urls>'] });
            if (!ok) return null;
            const list = await chrome.cookies.getAll({ url });
            const header = list.map((c) => `${c.name}=${c.value}`).join('; ');
            return header ? header.slice(0, 16 * 1024) : null;
        } catch (e) {
            return null;
        }
    }

    setupCapture() {
        // onCreated fires for every download (also when DevTools controls downloads) and has
        // the type and size; onDeterminingFilename adds the final name in normal browsing.
        // Each download is considered once, at whichever event comes first.
        chrome.downloads.onCreated.addListener((item) => {
            this.maybeTakeOver(item).catch(() => {});
        });
        chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
            suggest(); // never keep the browser waiting
            this.maybeTakeOver(item).catch(() => {});
        });
    }

    async maybeTakeOver(item) {
        if (this.seen.has(item.id)) return;
        this.seen.add(item.id);
        if (item.state && item.state !== 'in_progress') return;
        const url = item.finalUrl || item.url;
        if (!/^https?:/i.test(url) || item.byExtensionId === chrome.runtime.id) return;
        if (this.resent.delete(url)) return;
        const clicked = this.bypass.get(item.url) || this.bypass.get(url);
        if (clicked && Date.now() - clicked < BYPASS_MS) return;
        if (!(await this.captureEnabled()) || !(await this.token())) return;
        if (!(await this.findAppPort()) || this.apiVersion < 3) return;
        const fileName = (item.filename || '').split(/[\\/]/).pop() || null;
        const fileSize = item.totalBytes > 0 ? item.totalBytes : (item.fileSize > 0 ? item.fileSize : null);
        const check = await this.request('/api/capture/check', { method: 'POST', body: { url, fileName, fileSize, mime: item.mime || null } });
        if (!check.ok || !check.data.capture) return;
        try {
            await chrome.downloads.cancel(item.id);
            await chrome.downloads.erase({ id: item.id });
        } catch (e) {
            return; // already finished: leave it to the browser
        }
        const body = { url, fileName, fileSize, mode: 'now' };
        if (item.referrer && /^https?:/i.test(item.referrer)) body.pageUrl = item.referrer;
        const cookies = await this.cookiesFor(url);
        if (cookies) body.cookies = cookies;
        const r = await this.request('/api/download', { method: 'POST', body });
        if (!r.ok) {
            // The app could not take it: give the download back to the browser.
            this.resent.add(url);
            chrome.downloads.download({ url });
            this.notify('SwiftHarryDM', `Could not take over the download: ${r.data.error || 'HTTP ' + r.status}`);
        }
    }

    async sendLinks(urls, pageUrl) {
        const unique = [...new Set(urls.filter((u) => /^https?:/i.test(u)))].slice(0, 5000);
        if (!unique.length) {
            this.notify('SwiftHarryDM', 'No links were found.');
            return { success: false };
        }
        const body = { urls: unique };
        if (pageUrl && /^https?:/i.test(pageUrl)) body.pageUrl = pageUrl;
        const r = await this.request('/api/links', { method: 'POST', body });
        if (!r.ok) {
            this.notify('Links not sent', r.status === 401 ? 'Connect the extension to SwiftHarryDM first.' : (r.data.error || `HTTP ${r.status}`));
            return { success: false };
        }
        return { success: true, count: r.data.count };
    }

    /** Title and real formats of a video, from the app (API 3). */
    async mediaInfo(url) {
        if (!(await this.findAppPort()) || this.apiVersion < 3) return { success: false };
        const r = await this.request('/api/media/info', { method: 'POST', body: { url }, timeoutMs: 120000 });
        return r.ok ? { success: true, info: r.data.info } : { success: false, error: r.data.error };
    }

    async jobs() {
        const r = await this.request('/api/jobs');
        return r.ok ? { success: true, jobs: r.data.jobs } : { success: false, status: r.status, error: r.data.error };
    }

    async handleDownload(d) {
        const body = {
            url: d.url,
            title: (d.title || '').slice(0, 300),
            format: d.format || 'best',
            mode: d.instantDownload ? 'now' : 'queue',
        };
        if (d.pageUrl) body.pageUrl = d.pageUrl;
        if (d.type) body.type = String(d.type).slice(0, 32);
        if (d.formatId) body.formatId = d.formatId;
        const r = await this.request('/api/download', { method: 'POST', body });
        if (r.status === 401) {
            this.notify('Pair SwiftHarryDM first', 'Click the SwiftHarryDM toolbar icon and connect to the app.');
            return { success: false, error: 'Not paired with the app' };
        }
        if (!r.ok) {
            const msg = r.data.error || `HTTP ${r.status}`;
            this.notify('Download not sent', msg);
            return { success: false, error: msg };
        }
        const status = r.data.status;
        const text = status === 'pending_confirmation' ? 'Confirm the download in the SwiftHarryDM window.'
            : status === 'queued' ? 'Added to the queue.' : 'Download started.';
        this.notify('SwiftHarryDM', `${body.title || body.url}: ${text}`);
        return { success: true, status, jobId: r.data.job_id, requestId: r.data.request_id, message: text };
    }

    notify(title, message) {
        chrome.notifications.create({ type: 'basic', iconUrl: 'icons/icon128.png', title, message: String(message) });
    }

    createContextMenu() {
        // removeAll first: the service worker restarts often and duplicate ids throw.
        chrome.contextMenus.removeAll(() => {
            chrome.contextMenus.create({ id: 'download-with-swiftharry', title: 'Download with SwiftHarryDM',
                                         contexts: ['video', 'audio', 'link'] });
            chrome.contextMenus.create({ id: 'download-page-media', title: 'Download all media from page',
                                         contexts: ['page'] });
            chrome.contextMenus.create({ id: 'download-all-links', title: 'Download all links with SwiftHarryDM',
                                         contexts: ['page'] });
            chrome.contextMenus.create({ id: 'download-selected-links', title: 'Download selected links with SwiftHarryDM',
                                         contexts: ['selection'] });
        });
    }

    setupMessageListener() {
        chrome.runtime.onMessage.addListener((req, sender, sendResponse) => {
            const handlers = {
                downloadMedia: () => this.handleDownload(req.data || {}),
                checkAppStatus: () => this.status(),
                pairStart: () => this.pairStart(),
                pairConfirm: () => this.pairConfirm(req.pairingId, req.code),
                unpair: () => this.unpair(),
                listJobs: () => this.jobs(),
                sendLinks: () => this.sendLinks(req.urls || [], req.pageUrl),
                mediaInfo: () => this.mediaInfo(req.url),
                bypassNext: async () => {
                    if (req.url) this.bypass.set(req.url, Date.now());
                    return { success: true };
                },
            };
            const h = handlers[req.action];
            if (!h) return false;
            h().then(sendResponse, (e) => sendResponse({ success: false, error: String(e) }));
            return true;
        });
    }
}

const backgroundManager = new BackgroundManager();

chrome.contextMenus.onClicked.addListener((info, tab) => {
    if (!tab || tab.id === undefined) return;
    if (info.menuItemId === 'download-with-swiftharry') {
        chrome.tabs.sendMessage(tab.id, { action: 'contextMenuDownload', context: info });
    } else if (info.menuItemId === 'download-page-media') {
        chrome.tabs.sendMessage(tab.id, { action: 'downloadPageMedia' });
    } else if (info.menuItemId === 'download-all-links' || info.menuItemId === 'download-selected-links') {
        chrome.tabs.sendMessage(tab.id, { action: 'collectLinks', selectionOnly: info.menuItemId === 'download-selected-links' });
    }
});
