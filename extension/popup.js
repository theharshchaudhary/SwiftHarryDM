// popup.js - status, pairing and the recent download list. All text is set with
// textContent: titles come from web pages and must never be parsed as HTML.
document.addEventListener('DOMContentLoaded', () => {
    const $ = (id) => document.getElementById(id);
    const statusEl = $('status');
    const pairBox = $('pairBox');
    const codeInput = $('pairCode');
    let pairingId = null;

    const send = (msg) => chrome.runtime.sendMessage(msg);

    function setStatus(text, ok) {
        statusEl.textContent = text;
        statusEl.className = 'status ' + (ok ? 'connected' : 'disconnected');
    }

    async function refresh() {
        const s = await send({ action: 'checkAppStatus' });
        if (!s || !s.connected) {
            setStatus('❌ SwiftHarryDM is not running', false);
            pairBox.classList.add('hidden');
            return;
        }
        if (!s.paired) {
            setStatus(`⚠️ App found (v${s.version}) - not connected yet`, false);
            pairBox.classList.remove('hidden');
            return;
        }
        setStatus(`✅ Connected to SwiftHarryDM v${s.version}`, true);
        pairBox.classList.add('hidden');
        renderJobs();
    }

    async function renderJobs() {
        const r = await send({ action: 'listJobs' });
        const list = $('downloadsList');
        list.replaceChildren();
        if (!r || !r.success || !r.jobs.length) {
            $('pendingDownloads').classList.add('hidden');
            return;
        }
        $('pendingDownloads').classList.remove('hidden');
        for (const job of r.jobs.slice(-8).reverse()) {
            const item = document.createElement('div');
            item.className = 'download-item';
            const title = document.createElement('strong');
            title.textContent = job.title;
            const line = document.createElement('div');
            line.textContent = `${job.state}${job.progress !== null ? ` · ${job.progress}%` : ''}`;
            item.append(title, line);
            list.append(item);
        }
    }

    $('pairStartBtn').addEventListener('click', async () => {
        const r = await send({ action: 'pairStart' });
        if (!r.success) {
            setStatus(`Pairing failed: ${r.error}`, false);
            return;
        }
        pairingId = r.pairingId;
        $('pairStep2').classList.remove('hidden');
        setStatus('Type the 6-digit code shown in the SwiftHarryDM window', false);
        codeInput.focus();
    });

    $('pairConfirmBtn').addEventListener('click', async () => {
        const r = await send({ action: 'pairConfirm', pairingId, code: codeInput.value });
        if (r.success) {
            $('pairStep2').classList.add('hidden');
            refresh();
        } else {
            setStatus(r.status === 401 ? 'Wrong code, try again' : `Pairing failed: ${r.error}`, false);
        }
    });

    $('unpairBtn').addEventListener('click', async () => {
        await send({ action: 'unpair' });
        refresh();
    });

    const toActiveTab = (msg) => chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
        if (tabs[0]) chrome.tabs.sendMessage(tabs[0].id, msg);
        window.close();
    });
    $('scanPageBtn').addEventListener('click', () => toActiveTab({ action: 'showMediaDetection' }));
    $('forceRescanBtn').addEventListener('click', () => toActiveTab({ action: 'forceRescan' }));
    $('downloadAllBtn').addEventListener('click', () => toActiveTab({ action: 'downloadPageMedia' }));
    statusEl.addEventListener('click', refresh);

    const capture = $('captureToggle');
    chrome.storage.local.get('swiftdmCapture').then(({ swiftdmCapture }) => { capture.checked = swiftdmCapture !== false; });
    capture.addEventListener('change', () => chrome.storage.local.set({ swiftdmCapture: capture.checked }));

    const cookiesBtn = $('cookiesBtn');
    const want = { permissions: ['cookies'], origins: ['<all_urls>'] };
    chrome.permissions.contains(want).then((ok) => { if (ok) cookiesBtn.textContent = 'Site cookies are sent with taken-over downloads'; });
    cookiesBtn.addEventListener('click', async () => {
        if (await chrome.permissions.request(want)) cookiesBtn.textContent = 'Site cookies are sent with taken-over downloads';
    });

    refresh();
    const timer = setInterval(refresh, 3000);
    window.addEventListener('unload', () => clearInterval(timer));
});
