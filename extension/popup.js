// popup.js - connection status, page actions, settings and recent downloads.
// All text is set with textContent: titles come from web pages and are never parsed as HTML.
document.addEventListener('DOMContentLoaded', () => {
    const $ = (id) => document.getElementById(id);
    const send = (msg) => chrome.runtime.sendMessage(msg);
    let pairingId = null;

    function setStatus(text, ok) {
        $('status').textContent = text;
        $('dot').classList.toggle('ok', !!ok);
    }

    async function refresh() {
        const s = await send({ action: 'checkAppStatus' });
        const connected = s && s.connected;
        const paired = connected && s.paired;
        if (!connected) setStatus('SwiftHarryDM is not running', false);
        else if (!paired) setStatus('SwiftHarryDM found, not connected yet', false);
        else setStatus(`Connected to SwiftHarryDM ${s.version}`, true);
        $('pairBox').classList.toggle('hidden', !connected || paired);
        $('actions').classList.toggle('hidden', !paired);
        $('unpairBtn').classList.toggle('hidden', !paired);
        if (paired) renderJobs();
    }

    async function renderJobs() {
        const r = await send({ action: 'listJobs' });
        const list = $('list');
        list.replaceChildren();
        const jobs = r && r.success ? r.jobs.slice(-6).reverse() : [];
        $('recent').classList.toggle('hidden', !jobs.length);
        for (const job of jobs) {
            const row = document.createElement('div');
            row.className = 'item';
            const name = document.createElement('span');
            name.textContent = job.title;
            const state = document.createElement('span');
            const word = { COMPLETED: 'Done', DOWNLOADING: 'Downloading', PAUSED: 'Stopped', FAILED: 'Error', QUEUED: 'Waiting' }[job.state] || job.state;
            state.textContent = job.state === 'DOWNLOADING' && job.progress != null ? `${job.progress}%` : word;
            row.append(name, state);
            list.append(row);
        }
    }

    $('pairStartBtn').addEventListener('click', async () => {
        const r = await send({ action: 'pairStart' });
        if (!r || !r.success) {
            setStatus(`Could not connect: ${(r && r.error) || 'no answer'}`, false);
            return;
        }
        pairingId = r.pairingId;
        $('pairStep2').classList.remove('hidden');
        setStatus('Type the code shown by SwiftHarryDM', false);
        $('pairCode').focus();
    });
    $('pairConfirmBtn').addEventListener('click', async () => {
        const r = await send({ action: 'pairConfirm', pairingId, code: $('pairCode').value });
        if (r && r.success) {
            $('pairStep2').classList.add('hidden');
            refresh();
        } else {
            setStatus(r && r.status === 401 ? 'Wrong code, try again' : `Could not connect: ${(r && r.error) || ''}`, false);
        }
    });
    $('unpairBtn').addEventListener('click', async () => {
        await send({ action: 'unpair' });
        refresh();
    });

    const toActiveTab = (msg) => chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
        if (tabs[0]) chrome.tabs.sendMessage(tabs[0].id, msg).catch(() => {});
        window.close();
    });
    $('videoBtn').addEventListener('click', () => toActiveTab({ action: 'downloadPageMedia' }));
    $('linksBtn').addEventListener('click', () => toActiveTab({ action: 'collectLinks', selectionOnly: false }));

    const capture = $('captureToggle');
    chrome.storage.local.get('swiftdmCapture').then(({ swiftdmCapture }) => { capture.checked = swiftdmCapture !== false; });
    capture.addEventListener('change', () => chrome.storage.local.set({ swiftdmCapture: capture.checked }));

    const want = { permissions: ['cookies'], origins: ['<all_urls>'] };
    const cookiesOn = () => { $('cookiesBtn').textContent = 'Site cookies are sent with taken-over downloads'; };
    chrome.permissions.contains(want).then((ok) => ok && cookiesOn());
    $('cookiesBtn').addEventListener('click', async () => {
        if (await chrome.permissions.request(want)) cookiesOn();
    });

    refresh();
    const timer = setInterval(refresh, 3000);
    addEventListener('unload', () => clearInterval(timer));
});
