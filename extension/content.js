// content.js - FIXED VERSION (Buttons Always Appear)
class MediaDetector {
    constructor() {
        this.mediaElements = new Map();
        this.downloadButtons = new Set();
        this.observer = null;
        this.init();
    }

    init() {
        console.log('🎯 SwiftHarryDM Media Detector Started');
        this.detectAllMedia();
        this.setupMutationObserver();
        this.setupMessageListener();
        this.setupPeriodicScan();
    }

    detectAllMedia() {
        console.log('🔍 Scanning for media elements...');
        
        // Clear previous
        this.mediaElements.clear();
        this.downloadButtons.forEach(btn => {
            if (btn.dataset.intervalId) clearInterval(Number(btn.dataset.intervalId));
            btn.remove();
        });
        this.downloadButtons.clear();

        // Detect ALL videos and audios (no filters)
        const videos = document.querySelectorAll('video');
        const audios = document.querySelectorAll('audio');
        
        console.log(`🎬 Found ${videos.length} videos, ${audios.length} audios`);
        
        videos.forEach((video, index) => {
            this.processVideoElement(video, `video_${index}`);
        });

        audios.forEach((audio, index) => {
            this.processAudioElement(audio, `audio_${index}`);
        });

        this.detectVideoLinks();
        
        console.log(`✅ Attached ${this.downloadButtons.size} download buttons`);
    }

    processVideoElement(video, id) {
        try {
            const sources = this.getVideoSources(video);
            if (sources.length > 0 || video.src) {
                this.mediaElements.set(id, {
                    type: 'video',
                    element: video,
                    sources: sources,
                    title: this.getMediaTitle(video) || document.title,
                    thumbnail: video.getAttribute('poster'),
                    duration: video.duration || null,
                    url: video.src || window.location.href
                });
                this.attachDownloadButton(video, id);
            }
        } catch (error) {
            console.log('Error processing video:', error);
        }
    }

    processAudioElement(audio, id) {
        try {
            const sources = this.getAudioSources(audio);
            if (sources.length > 0 || audio.src) {
                this.mediaElements.set(id, {
                    type: 'audio',
                    element: audio,
                    sources: sources,
                    title: this.getMediaTitle(audio) || document.title,
                    duration: audio.duration || null,
                    url: audio.src || window.location.href
                });
                this.attachDownloadButton(audio, id);
            }
        } catch (error) {
            console.log('Error processing audio:', error);
        }
    }

    getVideoSources(video) {
        const sources = [];
        
        // Direct src attribute
        if (video.src && !video.src.startsWith('blob:')) {
            sources.push({
                url: video.src,
                quality: 'best',
                type: this.getMimeType(video.src)
            });
        }

        // Source elements
        video.querySelectorAll('source').forEach(source => {
            if (source.src && !source.src.startsWith('blob:')) {
                sources.push({
                    url: source.src,
                    quality: source.getAttribute('quality') || 'unknown',
                    type: source.type || this.getMimeType(source.src)
                });
            }
        });

        // If no direct sources, use the page URL (for YouTube, etc.)
        if (sources.length === 0) {
            sources.push({
                url: window.location.href,
                quality: 'best',
                type: 'video'
            });
        }

        return sources;
    }

    getAudioSources(audio) {
        const sources = [];
        
        if (audio.src && !audio.src.startsWith('blob:')) {
            sources.push({
                url: audio.src,
                quality: 'best',
                type: this.getMimeType(audio.src)
            });
        }

        audio.querySelectorAll('source').forEach(source => {
            if (source.src && !source.src.startsWith('blob:')) {
                sources.push({
                    url: source.src,
                    quality: 'best',
                    type: source.type || this.getMimeType(source.src)
                });
            }
        });

        // If no direct sources, use the page URL
        if (sources.length === 0) {
            sources.push({
                url: window.location.href,
                quality: 'best',
                type: 'audio'
            });
        }

        return sources;
    }

    getMediaTitle(mediaElement) {
        // Try multiple methods to get title
        const title = mediaElement.getAttribute('title') || 
                     mediaElement.getAttribute('alt') ||
                     mediaElement.getAttribute('aria-label') ||
                     document.title ||
                     'Media Download';
        
        return title.substring(0, 100); // Limit length
    }

    getMimeType(url) {
        if (!url) return 'unknown';
        const ext = url.split('.').pop().split('?')[0].toLowerCase();
        const mimeTypes = {
            'mp4': 'video/mp4',
            'webm': 'video/webm',
            'ogg': 'video/ogg',
            'mp3': 'audio/mp3',
            'wav': 'audio/wav',
            'm4a': 'audio/mp4'
        };
        return mimeTypes[ext] || 'unknown';
    }

    attachDownloadButton(element, mediaId) {
        // Remove existing button if any
        if (element.swiftharryButton) {
            element.swiftharryButton.remove();
        }

        const button = this.createDownloadButton();
        this.positionButtonOnMedia(element, button);
        
        button.addEventListener('click', (e) => {
            e.preventDefault();
            e.stopPropagation();
            this.showDownloadPopup(mediaId);
        });

        element.swiftharryButton = button;
        this.downloadButtons.add(button);
        
        // Make button more visible
        setTimeout(() => {
            button.style.opacity = '1';
            button.style.transform = 'translateY(0)';
        }, 100);
    }

    createDownloadButton() {
        const button = document.createElement('div');
        button.className = 'swiftharry-download-btn';
        button.style.cssText = `
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: white;
            border: none;
            border-radius: 20px;
            padding: 8px 16px;
            font-size: 12px;
            font-weight: 600;
            cursor: pointer;
            display: flex;
            align-items: center;
            gap: 6px;
            box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
            transition: all 0.3s ease;
            backdrop-filter: blur(10px);
            border: 1px solid rgba(255, 255, 255, 0.2);
            opacity: 0;
            transform: translateY(-10px);
            pointer-events: auto;
            z-index: 10000;
            position: fixed;
        `;
        button.innerHTML = `
            <svg width="14" height="14" viewBox="0 0 24 24" fill="white">
                <path d="M19 9h-4V3H9v6H5l7 7 7-7zM5 18v2h14v-2H5z"/>
            </svg>
            <span>Download</span>
        `;
        return button;
    }

    positionButtonOnMedia(mediaElement, button) {
        const updatePosition = () => {
            const rect = mediaElement.getBoundingClientRect();
            const scrollX = window.scrollX || window.pageXOffset;
            const scrollY = window.scrollY || window.pageYOffset;
            
            if (rect.width > 0 && rect.height > 0 && 
                rect.top < window.innerHeight && rect.bottom > 0) {
                
                // Position in top-right corner of media element
                button.style.top = (rect.top + scrollY + 10) + 'px';
                button.style.left = (rect.left + scrollX + rect.width - 120) + 'px';
                button.style.display = 'block';

            } else {
                button.style.display = 'none';
            }
        };

        updatePosition();
        document.body.appendChild(button);

        // Update position frequently
        const positionInterval = setInterval(updatePosition, 500);
        
        // Cleanup when button is removed
        button.dataset.intervalId = positionInterval;
    }

    showDownloadPopup(mediaId) {
        const media = this.mediaElements.get(mediaId);
        if (!media) {
            this.showErrorNotification('Media not found');
            return;
        }

        const popup = this.createDownloadPopup(media);
        document.body.appendChild(popup);

        const closeHandler = (e) => {
            if (!popup.contains(e.target)) {
                popup.remove();
                document.removeEventListener('click', closeHandler);
            }
        };
        
        setTimeout(() => {
            document.addEventListener('click', closeHandler);
        }, 100);
    }

    createDownloadPopup(media) {
        const popup = document.createElement('div');
        popup.className = 'swiftharry-download-popup';
        popup.style.cssText = `
            position: fixed;
            top: 50%;
            left: 50%;
            transform: translate(-50%, -50%);
            background: white;
            border-radius: 12px;
            box-shadow: 0 10px 30px rgba(0,0,0,0.3);
            z-index: 100000;
            min-width: 350px;
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            animation: popupAppear 0.3s ease;
        `;
        
        // Add CSS animation
        if (!document.querySelector('#swiftharry-animations')) {
            const style = document.createElement('style');
            style.id = 'swiftharry-animations';
            style.textContent = `
                @keyframes popupAppear {
                    from { opacity: 0; transform: translate(-50%, -40%); }
                    to { opacity: 1; transform: translate(-50%, -50%); }
                }
            `;
            document.head.appendChild(style);
        }
        
        popup.innerHTML = `
            <div class="popup-header" style="background: linear-gradient(135deg, #667eea 0%, #764ba2 100%); color: white; padding: 15px 20px; border-radius: 12px 12px 0 0; display: flex; justify-content: space-between; align-items: center;">
                <h3 style="margin: 0; font-size: 16px; font-weight: 600;">🚀 Download with SwiftHarryDM</h3>
                <button class="close-btn" style="background: none; border: none; color: white; font-size: 20px; cursor: pointer; padding: 0; width: 24px; height: 24px; display: flex; align-items: center; justify-content: center;">&times;</button>
            </div>
            <div class="popup-content" style="padding: 20px;">
                <div class="media-info" style="margin-bottom: 15px; padding: 10px; background: #f8f9fa; border-radius: 6px; font-size: 14px;">
                    <strong class="swiftharry-title" style="display: block; margin-bottom: 5px; color: #333;"></strong>
                    ${media.duration ? `<div style="color: #666;">Duration: ${this.formatDuration(media.duration)}</div>` : ''}
                    <div style="color: #666; font-size: 12px; margin-top: 5px;">Source: ${media.sources?.[0]?.url ? 'Direct' : 'Page URL'}</div>
                </div>
                <div class="format-selection" style="margin-bottom: 20px;">
                    <label style="display: block; margin-bottom: 8px; font-weight: 600; color: #333;">Download Format:</label>
                    <select class="format-select" style="width: 100%; padding: 10px; border: 2px solid #e9ecef; border-radius: 6px; font-size: 14px; background: white;">
                        <option value="best">Best Quality</option>
                        <option value="1080">1080p</option>
                        <option value="720">720p</option>
                        <option value="480">480p</option>
                        <option value="mp3">MP3 Audio</option>
                        <option value="m4a">M4A Audio</option>
                    </select>
                </div>
                <button class="download-now-btn" style="width: 100%; padding: 12px; background: linear-gradient(135deg, #10b981 0%, #059669 100%); color: white; border: none; border-radius: 6px; font-size: 14px; font-weight: 600; cursor: pointer; margin-bottom: 8px; transition: all 0.3s;">
                    ⬇️ Download Now
                </button>
                <button class="add-to-queue-btn" style="width: 100%; padding: 12px; background: linear-gradient(135deg, #3b82f6 0%, #1d4ed8 100%); color: white; border: none; border-radius: 6px; font-size: 14px; font-weight: 600; cursor: pointer; transition: all 0.3s;">
                    📥 Add to Queue
                </button>
            </div>
        `;

        popup.querySelector('.swiftharry-title').textContent = media.title || document.title;
        popup.querySelector('.close-btn').addEventListener('click', () => popup.remove());
        this.fillFormats(popup.querySelector('.format-select'), media);
        
        popup.querySelector('.download-now-btn').addEventListener('click', () => {
            this.startDownload(media, popup.querySelector('.format-select').value, true);
            popup.remove();
        });
        
        popup.querySelector('.add-to-queue-btn').addEventListener('click', () => {
            this.startDownload(media, popup.querySelector('.format-select').value, false);
            popup.remove();
        });

        // Add hover effects
        const downloadBtn = popup.querySelector('.download-now-btn');
        const queueBtn = popup.querySelector('.add-to-queue-btn');
        
        downloadBtn.addEventListener('mouseenter', () => {
            downloadBtn.style.transform = 'translateY(-1px)';
        });
        downloadBtn.addEventListener('mouseleave', () => {
            downloadBtn.style.transform = 'translateY(0)';
        });
        
        queueBtn.addEventListener('mouseenter', () => {
            queueBtn.style.transform = 'translateY(-1px)';
        });
        queueBtn.addEventListener('mouseleave', () => {
            queueBtn.style.transform = 'translateY(0)';
        });

        return popup;
    }

    // Ask the app for the video's real formats and add them under the presets. Everything
    // from the page or the server is set as text, never parsed as HTML.
    async fillFormats(select, media) {
        const url = this.cleanVideoUrl(media.sources?.[0]?.url || media.url || window.location.href);
        const loading = document.createElement('option');
        loading.disabled = true;
        loading.textContent = 'Loading the formats from SwiftHarryDM…';
        select.append(loading);
        let r;
        try {
            r = await chrome.runtime.sendMessage({ action: 'mediaInfo', url });
        } catch (e) {
            r = null;
        }
        loading.remove();
        const formats = r && r.success && r.info ? r.info.formats : [];
        if (!formats.length) return;
        const size = (n) => (n ? (n >= 1 << 30 ? `${(n / (1 << 30)).toFixed(2)} GB` : `${Math.round(n / (1 << 20))} MB`) : '');
        const group = document.createElement('optgroup');
        group.label = 'Exact formats';
        for (const f of formats.slice(0, 60)) {
            const o = document.createElement('option');
            o.value = `f:${f.id}`;
            const kind = f.vcodec && !f.acodec ? 'video only, best audio added' : !f.vcodec ? 'audio' : '';
            o.textContent = [f.height ? `${f.height}p` : 'audio', f.ext, kind, size(f.size), f.note].filter(Boolean).join(' · ');
            group.append(o);
        }
        select.append(group);
    }

    async startDownload(media, format, instant = false) {
        // Use the first source URL or fallback to page URL
        let downloadUrl = media.sources?.[0]?.url || media.url || window.location.href;
    
        // CRITICAL FIX: Clean YouTube URLs to remove playlist parameters
        downloadUrl = this.cleanVideoUrl(downloadUrl);
    
        let formatId = null;
        if (typeof format === 'string' && format.startsWith('f:')) {
            formatId = format.slice(2);
            format = 'best';
        }
        const downloadData = {
            url: downloadUrl,
            title: media.title,
            format: format,
            formatId,
            type: media.type,
            thumbnail: media.thumbnail,
            duration: media.duration,
            pageUrl: window.location.href,
            instantDownload: instant
        };

        console.log('📤 Starting download:', downloadData);

        try {
            const response = await chrome.runtime.sendMessage({
            action: 'downloadMedia',
            data: downloadData
        });
        
        console.log('📥 Extension response:', response);
        
        if (response && response.success) {
            this.showNotification('✅ Sent to SwiftHarryDM', `"${media.title || downloadUrl}" - ${response.message}`, '#10b981');
        } else {
            this.showErrorNotification(response?.error || 'Download failed');
        }
    } catch (error) {
        console.error('❌ Download error:', error);
        this.showErrorNotification('Make sure SwiftHarryDM app is running');
    }
}

// ADD THIS NEW METHOD to clean YouTube URLs
cleanVideoUrl(url) {
    console.log('🔧 Cleaning URL:', url);
    
    // Handle YouTube watch URLs
    if (url.includes('youtube.com/watch') && url.includes('v=')) {
        try {
            const urlObj = new URL(url);
            const videoId = urlObj.searchParams.get('v');
            
            if (videoId) {
                // Create clean URL with ONLY the video ID
                const cleanUrl = `https://www.youtube.com/watch?v=${videoId}`;
                console.log('✅ Cleaned YouTube URL:', cleanUrl);
                return cleanUrl;
            }
        } catch (error) {
            console.error('❌ URL cleaning failed:', error);
        }
    }
    
    // Handle YouTube short URLs
    else if (url.includes('youtu.be/')) {
        try {
            const urlObj = new URL(url);
            const videoId = urlObj.pathname.replace('/', '');
            
            if (videoId) {
                const cleanUrl = `https://www.youtube.com/watch?v=${videoId}`;
                console.log('✅ Converted short URL:', cleanUrl);
                return cleanUrl;
            }
        } catch (error) {
            console.error('❌ Short URL conversion failed:', error);
        }
    }
    
    // Handle other video platforms if needed
    else if (url.includes('vimeo.com/')) {
        // Clean Vimeo URLs if necessary
        console.log('🎬 Vimeo URL detected:', url);
    }
    
    // Return original URL if no cleaning needed
    console.log('🔧 Using original URL:', url);
    return url;
}

    showSuccessNotification(title, queuePosition) {
        this.showNotification(
            '✅ Download Started', 
            `"${title}" - Queue: ${queuePosition}`, 
            '#10b981'
        );
    }

    showErrorNotification(message) {
        this.showNotification('❌ Download Failed', message, '#ef4444');
    }

    showNotification(title, message, color) {
        // Remove existing notification
        const existing = document.getElementById('swiftharry-notification');
        if (existing) existing.remove();

        const notification = document.createElement('div');
        notification.id = 'swiftharry-notification';
        notification.style.cssText = `
            position: fixed;
            top: 20px;
            right: 20px;
            background: ${color};
            color: white;
            padding: 15px 20px;
            border-radius: 8px;
            box-shadow: 0 4px 12px rgba(0,0,0,0.3);
            z-index: 100001;
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            max-width: 300px;
            animation: slideIn 0.3s ease;
        `;
        
        // Add slideIn animation
        if (!document.querySelector('#swiftharry-notification-animations')) {
            const style = document.createElement('style');
            style.id = 'swiftharry-notification-animations';
            style.textContent = `
                @keyframes slideIn {
                    from { transform: translateX(100%); opacity: 0; }
                    to { transform: translateX(0); opacity: 1; }
                }
            `;
            document.head.appendChild(style);
        }
        
        const head = document.createElement('div');
        head.style.cssText = 'font-weight: 600; margin-bottom: 5px;';
        head.textContent = title;
        const body = document.createElement('div');
        body.style.cssText = 'font-size: 14px;';
        body.textContent = message;
        notification.append(head, body);
        
        document.body.appendChild(notification);
        
        setTimeout(() => {
            if (notification.parentNode) {
                notification.remove();
            }
        }, 4000);
    }

    formatDuration(seconds) {
        if (!seconds || isNaN(seconds)) return 'Unknown';
        const mins = Math.floor(seconds / 60);
        const secs = Math.floor(seconds % 60);
        return `${mins}:${secs.toString().padStart(2, '0')}`;
    }

    setupMutationObserver() {
        this.observer = new MutationObserver((mutations) => {
            let shouldRescan = false;
            
            mutations.forEach((mutation) => {
                // Check for added nodes
                mutation.addedNodes.forEach((node) => {
                    if (node.nodeType === 1) { // Element node
                        if (node.tagName === 'VIDEO' || node.tagName === 'AUDIO') {
                            shouldRescan = true;
                        }
                        if (node.querySelectorAll) {
                            const mediaElements = node.querySelectorAll('video, audio');
                            if (mediaElements.length > 0) {
                                shouldRescan = true;
                            }
                        }
                    }
                });
                
                // Check for attribute changes (like when video becomes visible)
                if (mutation.type === 'attributes') {
                    if (mutation.target.tagName === 'VIDEO' || mutation.target.tagName === 'AUDIO') {
                        shouldRescan = true;
                    }
                }
            });
            
            if (shouldRescan) {
                console.log('🔄 DOM changed, rescanning for media...');
                setTimeout(() => this.detectAllMedia(), 1000);
            }
        });

        this.observer.observe(document.body, {
            childList: true,
            subtree: true,
            attributes: true,
            attributeFilter: ['style', 'class', 'src', 'poster']
        });
    }

    setupPeriodicScan() {
        // Rescan every 3 seconds to catch dynamically loaded media
        setInterval(() => {
            const currentMediaCount = document.querySelectorAll('video, audio').length;
            if (currentMediaCount !== this.mediaElements.size) {
                console.log('🔄 Periodic rescan triggered');
                this.detectAllMedia();
            }
        }, 3000);
    }

    setupMessageListener() {
        chrome.runtime.onMessage.addListener((request, sender, sendResponse) => {
            console.log('Content script received:', request);
            
            switch (request.action) {
                case 'contextMenuDownload':
                    this.handleContextMenuDownload(request.context);
                    break;
                case 'downloadPageMedia':
                    this.downloadAllMedia();
                    break;
                case 'showMediaDetection':
                    this.showDetectionResults();
                    break;
                case 'collectLinks':
                    this.sendLinks(request.selectionOnly);
                    break;
                case 'forceRescan':
                    this.detectAllMedia();
                    this.showNotification('🔍 Rescan Complete', `Found ${this.mediaElements.size} media elements`, '#3b82f6');
                    break;
            }
        });
    }

    // Links of the whole page, or only those inside the current selection.
    sendLinks(selectionOnly) {
        let anchors = [...document.querySelectorAll('a[href]')];
        if (selectionOnly) {
            const sel = window.getSelection();
            anchors = anchors.filter((a) => sel && sel.rangeCount && sel.containsNode(a, true));
        }
        const urls = anchors.map((a) => a.href).filter((h) => /^https?:/i.test(h));
        chrome.runtime.sendMessage({ action: 'sendLinks', urls, pageUrl: location.href });
    }

    handleContextMenuDownload(context) {
    if (context.mediaType === 'video' || context.mediaType === 'audio') {
        this.showMediaSelectionPopup();
    } else if (context.linkUrl) {
        // Clean the URL before downloading
        const cleanUrl = this.cleanVideoUrl(context.linkUrl);
        
        this.startDownload({
            url: cleanUrl,
            title: context.linkText || 'Linked Media',
            type: 'video_link'
        }, 'best', true);
    }
}

    downloadAllMedia() {
        if (this.mediaElements.size === 0) {
            this.showErrorNotification('No media found to download');
            return;
        }
        
        let count = 0;
        this.mediaElements.forEach((media, id) => {
            if (media.sources && media.sources.length > 0) {
                this.startDownload(media, 'best', false);
                count++;
            }
        });
        
        this.showNotification('✅ Sent to SwiftHarryDM', `${count} downloads sent - check the app`, '#10b981');
    }

    showDetectionResults() {
        const count = this.mediaElements.size;
        this.showNotification(
            '🎯 Media Detection', 
            `Found ${count} media elements on this page`, 
            '#3b82f6'
        );
    }

    detectVideoLinks() {
    // Simple video link detection for common platforms
    const patterns = [
        /youtube\.com\/watch\?v=/i,
        /youtu\.be\//i,
        /vimeo\.com\//i,
        /dailymotion\.com\//i
    ];

    // Check if current page is a video page
    const currentUrl = window.location.href;
    if (patterns.some(pattern => pattern.test(currentUrl))) {
        // Clean the URL for the page video
        const cleanUrl = this.cleanVideoUrl(currentUrl);
        
        const pageMedia = {
            type: 'video_page',
            url: cleanUrl, // Use cleaned URL instead of original
            title: document.title,
            element: document.body
        };
        this.mediaElements.set('page_video', pageMedia);
        
        // Add download button to page
        const pageButton = this.createDownloadButton();
        pageButton.innerHTML = `<svg width="14" height="14" viewBox="0 0 24 24" fill="white"><path d="M19 9h-4V3H9v6H5l7 7 7-7zM5 18v2h14v-2H5z"/></svg><span>Download Page</span>`;
        pageButton.style.position = 'fixed';
        pageButton.style.top = '20px';
        pageButton.style.right = '20px';
        pageButton.style.zIndex = '10000';
        pageButton.addEventListener('click', () => {
            this.showDownloadPopup('page_video');
        });
        document.body.appendChild(pageButton);
        this.downloadButtons.add(pageButton);
    }
}
}

// Initialize immediately
console.log('🚀 Loading SwiftHarryDM Media Detector...');
new MediaDetector();

// Also re-initialize when page fully loads
window.addEventListener('load', () => {
    console.log('📄 Page fully loaded, rescanning for media...');
    setTimeout(() => {
        if (window.MediaDetectorInstance) {
            window.MediaDetectorInstance.detectAllMedia();
        }
    }, 2000);
});
// Alt+click on a link leaves that download to the browser.
document.addEventListener('mousedown', (e) => {
    if (!e.altKey) return;
    const a = e.target && e.target.closest ? e.target.closest('a[href]') : null;
    if (a && /^https?:/i.test(a.href)) chrome.runtime.sendMessage({ action: 'bypassNext', url: a.href });
}, true);
