// background.js - talks to the SwiftHarryDM desktop app on 127.0.0.1 (API v2).
// The app only accepts requests carrying a token obtained by pairing: the app shows
// a one-time code, the user types it into this extension's popup.
const PORTS = [5001, 5002];

class BackgroundManager {
    constructor() {
        this.appPort = null;
        this.createContextMenu();
        this.setupMessageListener();
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
    }
});
