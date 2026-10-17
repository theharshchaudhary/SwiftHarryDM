import sys

# Must run before anything prints: redirected stdout under cp1252 crashed every
# download on emoji output (audit T0).
from swiftdm.compat import harden_stdio
harden_stdio()

# Packaged builds launch themselves as the yt-dlp subprocess used by the v2 media engine.
if len(sys.argv) > 1 and sys.argv[1] == "--yt-dlp-shim":
    import yt_dlp
    sys.exit(yt_dlp.main(sys.argv[2:]))

# Engine switch (--engine flag > SWIFTDM_ENGINE > settings). The legacy engine below
# stays available for rollback until the documented removal criteria are met.
if __name__ == "__main__":
    import os as _os
    from swiftdm.config.settings import SettingsStore as _SettingsStore, resolve_engine as _resolve_engine
    if _resolve_engine(sys.argv, _os.environ, _SettingsStore().get()) == "v2":
        from swiftdm.app import run as _run_v2
        sys.exit(_run_v2(sys.argv))

import tkinter as tk
from tkinter import ttk, messagebox, simpledialog, filedialog
import threading
import os
import tarfile
import zipfile
from tqdm import tqdm
import re
import requests
import json
import hashlib
import uuid
import time
import subprocess
import platform
import winreg
from datetime import datetime, timedelta
from src.downloader import Downloader
from utils import get_ffmpeg_path, convert_to_mp3, manual_merge_video_audio
from utils import convert_to_mp3, format_mapping
from yt_dlp import YoutubeDL
from flask import Flask, request, jsonify
from download_window import show_download_window, close_download_window, active_download_windows
from swiftdm.logging_setup import redact_url


# ------------------ Globals ------------------
download_queue = []
paused_flags = []
queue_frames = []
CURRENT_VERSION = "1.0.0"
UPDATE_CHECK_URL = "https://swiftharrydm.harshchaudhary.com.np/version.txt"
DOWNLOAD_PAGE_URL = "https://swiftharrydm.harshchaudhary.com.np/downloads"

# License / Trial Config
LICENSE_SERVER = "https://swift.harshchaudhary.com.np/"
TOKEN_FILE = os.path.join(os.path.expanduser("~"), ".swift_dm_token.json")
TRIAL_DAYS = 7

# ------------------ IDM Color Theme ------------------
COLORS = {
    "primary": "#2C5F8A",        # IDM Blue
    "primary_light": "#3A7BB3",  # Lighter Blue
    "primary_dark": "#1E4A6B",   # Darker Blue
    "secondary": "#E8A735",      # Accent Orange
    "success": "#4CAF50",        # Green
    "warning": "#FF9800",        # Orange
    "danger": "#F44336",         # Red
    "dark_bg": "#2D2D2D",        # Dark Background
    "dark_surface": "#3C3C3C",   # Dark Surface
    "dark_text": "#FFFFFF",      # White Text
    "light_text": "#CCCCCC",     # Light Gray Text
    "border": "#555555",         # Border Color
    "header_bg": "#1E3A5F",      # Header Background
    "row_even": "#2D2D2D",       # Even Row
    "row_odd": "#363636",        # Odd Row
    "progress_bg": "#1E4A6B",    # Progress Background
    "progress_fg": "#4CAF50",    # Progress Foreground
}

# ------------------ FFMPEG Integration ------------------
def download_and_setup_ffmpeg():
    """Automatically download and setup FFmpeg"""
    print("🔧 Setting up FFmpeg automatically...")
    
    # Determine platform
    system = platform.system().lower()
    arch = platform.machine().lower()
    
    ffmpeg_dir = os.path.join(os.path.expanduser("~"), "ffmpeg")
    os.makedirs(ffmpeg_dir, exist_ok=True)
    
    # Download URLs for different platforms
    urls = {
        'windows': {
            'x86_64': 'https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip',
            'amd64': 'https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip'
        },
        'linux': {
            'x86_64': 'https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz'
        },
        'darwin': {
            'x86_64': 'https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-macos64-gpl.tar.xz'
        }
    }
    
    if system not in urls:
        print(f"❌ Unsupported platform: {system}")
        return False
    
    url = urls[system].get(arch, urls[system]['x86_64'])
    download_path = os.path.join(ffmpeg_dir, "ffmpeg_download")
    
    try:
        # Download FFmpeg
        print(f"📥 Downloading FFmpeg from: {url}")
        response = requests.get(url, stream=True)
        total_size = int(response.headers.get('content-length', 0))
        
        with open(download_path, 'wb') as file, tqdm(
            desc="Downloading FFmpeg",
            total=total_size,
            unit='iB',
            unit_scale=True,
            unit_divisor=1024,
        ) as bar:
            for data in response.iter_content(chunk_size=1024):
                size = file.write(data)
                bar.update(size)
        
        # Extract based on file type
        print("📦 Extracting FFmpeg...")
        if download_path.endswith('.zip'):
            with zipfile.ZipFile(download_path, 'r') as zip_ref:
                zip_ref.extractall(ffmpeg_dir)
        elif download_path.endswith('.tar.xz'):
            with tarfile.open(download_path, 'r:xz') as tar_ref:
                tar_ref.extractall(ffmpeg_dir)
        else:
            print(f"❌ Unsupported archive format: {download_path}")
            return False
        
        # Clean up download file
        os.remove(download_path)
        
        # Find ffmpeg executable
        ffmpeg_exe = None
        for root, dirs, files in os.walk(ffmpeg_dir):
            for file in files:
                if file in ['ffmpeg.exe', 'ffmpeg']:
                    ffmpeg_exe = os.path.join(root, file)
                    break
            if ffmpeg_exe:
                break
        
        if not ffmpeg_exe:
            print("❌ Could not find ffmpeg executable after extraction")
            return False
        
        print(f"✅ FFmpeg installed at: {ffmpeg_exe}")
        return ffmpeg_exe
        
    except Exception as e:
        print(f"❌ Failed to setup FFmpeg automatically: {e}")
        return False

def setup_ffmpeg_automatically():
    """Setup FFmpeg automatically with user confirmation"""
    try:
        response = messagebox.askyesno(
            "FFmpeg Required", 
            "FFmpeg is required for MP3 conversion and video processing.\n\n"
            "Do you want to download and install FFmpeg automatically?\n\n"
            "This will download ~50MB and install it in your home directory."
        )
        
        if response:
            ffmpeg_path = download_and_setup_ffmpeg()
            if ffmpeg_path:
                messagebox.showinfo("Success", f"FFmpeg installed successfully!\n\nLocation: {ffmpeg_path}")
                return True
            else:
                messagebox.showerror("Error", "Failed to install FFmpeg automatically.\n\nPlease install it manually.")
                return False
        return False
    except Exception as e:
        print(f"❌ Automatic FFmpeg setup failed: {e}")
        return False

# ------------------ IDM Style Configuration ------------------
def configure_idm_styles():
    style = ttk.Style()
    
    # Configure main styles
    style.configure("IDM.TFrame", background=COLORS["dark_bg"])
    style.configure("IDM.TLabel", background=COLORS["dark_bg"], foreground=COLORS["dark_text"])
    style.configure("IDM.TButton", 
                   background=COLORS["primary"],
                   foreground=COLORS["dark_text"],
                   borderwidth=1,
                   focuscolor=COLORS["primary_light"])
    
    # Progressbar style
    style.configure("IDM.Horizontal.TProgressbar",
                   background=COLORS["progress_fg"],
                   troughcolor=COLORS["progress_bg"],
                   borderwidth=0,
                   lightcolor=COLORS["progress_fg"],
                   darkcolor=COLORS["progress_fg"])
    
    # Treeview style (for download list)
    style.configure("IDM.Treeview",
                   background=COLORS["dark_surface"],
                   foreground=COLORS["dark_text"],
                   fieldbackground=COLORS["dark_surface"],
                   borderwidth=0,
                   rowheight=25)
    
    style.configure("IDM.Treeview.Heading",
                   background=COLORS["primary_dark"],
                   foreground=COLORS["dark_text"],
                   relief="flat",
                   borderwidth=0,
                   font=('Arial', 10, 'bold'))
    
    # Combobox style
    style.configure("IDM.TCombobox",
                   background=COLORS["dark_surface"],
                   foreground=COLORS["dark_text"],
                   fieldbackground=COLORS["dark_surface"],
                   borderwidth=1)
    
    # Scrollbar style
    style.configure("IDM.Vertical.TScrollbar",
                   background=COLORS["primary_dark"],
                   troughcolor=COLORS["dark_bg"],
                   borderwidth=0,
                   arrowsize=12)

# ------------------ Utilities ------------------
def clean_percent(percent_str):
    ansi_escape = re.compile(r'\x1B[@-_][0-?]*[ -/]*[@-~]')
    clean_str = ansi_escape.sub('', percent_str)
    try:
        return int(float(clean_str.replace('%','').strip()))
    except:
        return 0

def sanitize_filename(name):
    # More comprehensive sanitization
    name = re.sub(r'[<>:"/\\|?*]', '_', name)
    name = re.sub(r'[\x00-\x1f\x7f-\x9f]', '', name)  # Remove control characters
    name = name.strip().strip('.')  # Remove leading/trailing spaces and dots
    name = name[:200]  # Limit length
    return name if name else "download"

def open_download_folder():
    """Open the download folder in file explorer"""
    try:
        save_path = get_default_save_path()
        if os.path.exists(save_path):
            os.startfile(save_path)  # Windows
            log_text.insert(tk.END, f"📁 Opened folder: {save_path}\n")
        else:
            messagebox.showinfo("Folder Not Found", f"Download folder doesn't exist:\n{save_path}")
    except Exception as e:
        messagebox.showerror("Error", f"Cannot open folder:\n{str(e)}")

def get_filename_from_url(url):
    return sanitize_filename(url.split('/')[-1])

def get_machine_id():
    """Get persistent Windows Machine ID that survives resets and reformats"""
    machine_id_file = os.path.join(os.path.expanduser("~"), ".swift_dm_machine_id")
    
    # Try to load stored machine ID first
    if os.path.exists(machine_id_file):
        try:
            with open(machine_id_file, "r") as f:
                stored_id = f.read().strip()
                if len(stored_id) == 64:  # SHA256 length
                    return stored_id
        except:
            pass
    
    # Generate new persistent machine ID using Windows-specific identifiers
    try:
        # Method 1: Windows Machine GUID (Most Stable - survives resets/reformats)
        try:
            key = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Cryptography")
            machine_guid, _ = winreg.QueryValueEx(key, "MachineGuid")
            winreg.CloseKey(key)
            base_id = f"WIN_GUID_{machine_guid}"
            print("🔧 Using Windows Machine GUID")
        except Exception as e:
            print(f"❌ Failed to get MachineGuid: {e}")
            base_id = None
        
        # Method 2: BIOS UUID (Very stable - hardware level)
        if not base_id:
            try:
                result = subprocess.run(
                    ['wmic', 'csproduct', 'get', 'UUID'], 
                    capture_output=True, 
                    text=True, 
                    creationflags=subprocess.CREATE_NO_WINDOW
                )
                if result.returncode == 0:
                    bios_uuid = result.stdout.strip().split('\n')[1].strip()
                    if bios_uuid and bios_uuid != "":
                        base_id = f"BIOS_UUID_{bios_uuid}"
                        print(f"🔧 Using BIOS UUID: {bios_uuid}")
            except Exception as e:
                print(f"❌ Failed to get BIOS UUID: {e}")
        
        # Method 3: BaseBoard Serial (Motherboard serial)
        if not base_id:
            try:
                result = subprocess.run(
                    ['wmic', 'baseboard', 'get', 'serialnumber'], 
                    capture_output=True, 
                    text=True, 
                    creationflags=subprocess.CREATE_NO_WINDOW
                )
                if result.returncode == 0:
                    board_serial = result.stdout.strip().split('\n')[1].strip()
                    if board_serial and board_serial not in ['', 'None', 'To be filled by O.E.M.']:
                        base_id = f"BOARD_SN_{board_serial}"
                        print(f"🔧 Using BaseBoard Serial: {board_serial}")
            except Exception as e:
                print(f"❌ Failed to get BaseBoard Serial: {e}")
        
        # Method 4: CPU Serial (Processor serial)
        if not base_id:
            try:
                result = subprocess.run(
                    ['wmic', 'cpu', 'get', 'processorid'], 
                    capture_output=True, 
                    text=True, 
                    creationflags=subprocess.CREATE_NO_WINDOW
                )
                if result.returncode == 0:
                    cpu_id = result.stdout.strip().split('\n')[1].strip()
                    if cpu_id and cpu_id != "":
                        base_id = f"CPU_ID_{cpu_id}"
                        print(f"🔧 Using CPU ID: {cpu_id}")
            except Exception as e:
                print(f"❌ Failed to get CPU ID: {e}")
        
        # Method 5: Disk Drive Serial (Physical disk serial)
        if not base_id:
            try:
                result = subprocess.run(
                    ['wmic', 'diskdrive', 'where', 'index=0', 'get', 'serialnumber'], 
                    capture_output=True, 
                    text=True, 
                    creationflags=subprocess.CREATE_NO_WINDOW
                )
                if result.returncode == 0:
                    disk_serial = result.stdout.strip().split('\n')[1].strip()
                    if disk_serial and disk_serial != "":
                        base_id = f"DISK_SN_{disk_serial}"
                        print(f"🔧 Using Disk Serial: {disk_serial}")
            except Exception as e:
                print(f"❌ Failed to get Disk Serial: {e}")
        
        # Final fallback - combine multiple identifiers
        if not base_id:
            print("⚠️ Using fallback combined identifiers")
            hostname = platform.node()
            mac = uuid.getnode()
            base_id = f"FALLBACK_{hostname}_{mac}"
        
        # Create hash and store
        machine_id = hashlib.sha256(base_id.encode()).hexdigest()
        
        # Store it persistently
        try:
            with open(machine_id_file, "w") as f:
                f.write(machine_id)
            print(f"✅ Generated and stored persistent Machine ID: {machine_id[:16]}...")
        except Exception as e:
            print(f"❌ Failed to store machine ID: {e}")
            
        return machine_id
        
    except Exception as e:
        print(f"💥 Critical error generating machine ID: {e}")
        # Ultimate fallback
        fallback_id = hashlib.sha256("swiftharry_fallback_id".encode()).hexdigest()
        return fallback_id

def get_default_save_path():
    """Get reliable default save path with fallbacks"""
    try:
        # Try Desktop first
        desktop = os.path.join(os.path.expanduser("~"), "Desktop")
        if os.path.exists(desktop):
            save_path = os.path.join(desktop, "SwiftHarryDM Downloads")
        else:
            # Fallback to Documents
            documents = os.path.join(os.path.expanduser("~"), "Documents")
            save_path = os.path.join(documents, "SwiftHarryDM Downloads")
        
        # Ensure directory exists
        os.makedirs(save_path, exist_ok=True)
        return save_path
        
    except Exception as e:
        # Ultimate fallback - current directory
        fallback = os.path.join(os.getcwd(), "SwiftHarryDM Downloads")
        os.makedirs(fallback, exist_ok=True)
        return fallback

def save_local_token(data):
    # Always include machine_id and Windows metadata
    data.update({
        "machine_id": get_machine_id(),
        "windows_platform": True,
        "last_updated": datetime.now().isoformat(),
        "app_version": CURRENT_VERSION
    })
    
    with open(TOKEN_FILE, "w") as f:
        json.dump(data, f)

def load_local_token():
    if os.path.exists(TOKEN_FILE):
        with open(TOKEN_FILE, "r") as f:
            return json.load(f)
    return None

#--------------------Enhanced Extension Integration------------------------
app = Flask(__name__)

# No CORS headers: the extension's host_permissions already allow its requests, and
# a wildcard Access-Control-Allow-Origin let any website queue downloads (audit).
_ALLOWED_HOSTS = {f"{h}:{p}" for h in ("127.0.0.1", "localhost") for p in (5001, 5002)}

@app.before_request
def reject_untrusted_callers():
    # Host check blocks DNS-rebinding; Origin check blocks web pages.
    if request.host not in _ALLOWED_HOSTS:
        return jsonify({"success": False, "error": "forbidden host"}), 403
    origin = request.headers.get("Origin")
    if origin and not origin.startswith("chrome-extension://"):
        return jsonify({"success": False, "error": "forbidden origin"}), 403

@app.route("/health", methods=["GET", "OPTIONS"])
def health_check():
    if request.method == 'OPTIONS':
        return jsonify({"status": "ok"})
    return jsonify({
        "status": "running", 
        "app": "SwiftHarryDM",
        "version": CURRENT_VERSION,
        "timestamp": datetime.now().isoformat()
    })

@app.route("/api/download", methods=["POST", "OPTIONS"])
def handle_download():
    if request.method == 'OPTIONS':
        return jsonify({"status": "ok"})
    
    try:
        data = request.get_json()
        print(f"📥 [EXTENSION] Download request received")
        print(f"   URL: {redact_url(str(data.get('url', 'N/A')))}")
        print(f"   Title: {data.get('title', 'N/A')}")
        print(f"   Format: {data.get('format', 'N/A')}")
        
        url = data.get("url")
        title = data.get("title", "Unknown Title")
        format_type = data.get("format", "best")
        page_url = data.get("pageUrl", "")
        
        if not url:
            return jsonify({"success": False, "error": "No URL provided"}), 400
        
        # Sanitize title for filename
        title = sanitize_filename(title)
        
        # Default save path
        save_path = os.path.join(os.path.expanduser("~"), "Desktop", "SwiftHarryDM Downloads")
        os.makedirs(save_path, exist_ok=True)
        
        # Create download item - AUTO START ALL EXTENSION DOWNLOADS
        item = {
            "url": url, 
            "fmt": format_type, 
            "save_path": save_path, 
            "status": "Downloading",  # Changed from "Pending" to "Downloading"
            "progress": 0,
            "title": title,
            "source": "extension",
            "page_url": page_url,
            "added_at": datetime.now().isoformat()
        }
        
        # Add to queue
        download_queue.append(item)
        paused_flags.append(False)
        queue_position = len(download_queue)
        
        # Update GUI in main thread - FIXED with proper lambda
        root.after(0, lambda idx=len(download_queue)-1: add_extension_download_to_gui(idx))
        
        # AUTO START DOWNLOAD for all extension requests
        root.after(100, lambda idx=len(download_queue)-1: start_extension_download(idx))
        
        log_text.insert(tk.END, f"✅ [EXTENSION] Added and started: {title}\n")
        log_text.see(tk.END)
        
        return jsonify({
            "success": True, 
            "message": "Download added to queue and started",
            "queue_position": queue_position,
            "title": title,
            "format": format_type
        })
        
    except Exception as e:
        error_msg = f"❌ [EXTENSION] Error: {str(e)}"
        print(error_msg)
        log_text.insert(tk.END, f"{error_msg}\n")
        log_text.see(tk.END)
        return jsonify({"success": False, "error": str(e)}), 500

@app.route("/api/queue", methods=["GET", "OPTIONS"])
def get_queue_status():
    if request.method == 'OPTIONS':
        return jsonify({"status": "ok"})
    
    queue_info = []
    for idx, item in enumerate(download_queue):
        queue_info.append({
            "position": idx + 1,
            "title": item.get("title", get_filename_from_url(item["url"])),
            "status": item["status"],
            "progress": item["progress"],
            "format": item["fmt"]
        })
    
    return jsonify({
        "total": len(download_queue),
        "active": len([item for item in download_queue if item["status"] == "Downloading"]),
        "queue": queue_info
    })

def add_extension_download_to_gui(idx):
    """Add extension download to GUI queue"""
    if idx < len(download_queue):
        create_queue_item(idx)
        # Auto-scroll to show new item
        queue_canvas.yview_moveto(1.0)
        # Update queue count
        update_queue_count()
        
        # Log the addition
        item = download_queue[idx]
        log_text.insert(tk.END, f"📥 [EXTENSION] Added to queue: {item['title']} ({item['fmt']})\n")
        log_text.see(tk.END)
        
        print(f"✅ [EXTENSION] Added to GUI: {item['title']} at index {idx}")

def start_extension_download(idx):
    """Start download for extension-added items"""
    if idx < len(download_queue) and download_queue[idx]["status"] == "Downloading":
        print(f"🚀 [EXTENSION] Starting download for index {idx}: {download_queue[idx]['title']}")
        # Update the GUI first
        update_queue_item(idx)
        # Then start the download worker
        threading.Thread(target=download_worker, args=(idx,), daemon=True).start()

def run_flask():
    """Run Flask server for browser extension"""
    try:
        print("🚀 Starting Flask server for browser extension on port 5001...")
        # Disable Flask logging for cleaner output
        import logging
        log = logging.getLogger('werkzeug')
        log.setLevel(logging.ERROR)
        
        # Use these settings for better compatibility
        app.run(
            host='127.0.0.1', 
            port=5001, 
            debug=False, 
            threaded=True, 
            use_reloader=False
        )
    except Exception as e:
        print(f"❌ Flask server error on port 5001: {e}")
        # Try alternative port
        try:
            print("🔄 Trying port 5002...")
            app.run(
                host='127.0.0.1', 
                port=5002, 
                debug=False, 
                threaded=True, 
                use_reloader=False
            )
        except Exception as e2:
            print(f"❌ Failed to start Flask server: {e2}")

# Start Flask in background when app launches
threading.Thread(target=run_flask, daemon=True).start()

# ------------------ License & Trial ------------------
def start_trial(machine_id):
    try:
        print(f"🔄 Starting trial for machine: {machine_id[:8]}...")
        resp = requests.post(f"{LICENSE_SERVER}/trial", json={"machine_id": machine_id}, timeout=10)
        resp.raise_for_status()
        data = resp.json()
        print(f"📄 Trial response: {data}")
        
        if data.get("valid"):
            days_left = data.get("days_left", TRIAL_DAYS)
            trial_data = {
                "machine_id": machine_id, 
                "trial_start": datetime.now().isoformat(),
                "trial_start_server": data.get("trial_started"),
                "days_left": days_left
            }
            save_local_token(trial_data)
            license_status_var.set(f"Trial: {days_left} days left")
            messagebox.showinfo("Trial Started", f"Your trial started! {days_left} days left.")
            return True
        else:
            messagebox.showinfo("Trial Expired", "Trial expired. Enter a license key to continue.")
            return ask_for_license()
    except Exception as e:
        print(f"❌ Trial error: {e}")
        token = load_local_token()
        if token and "trial_start" in token:
            trial_start = datetime.fromisoformat(token["trial_start"])
            elapsed = (datetime.now() - trial_start).days
            if elapsed < TRIAL_DAYS:
                days_left = TRIAL_DAYS - elapsed
                license_status_var.set(f"Trial: {days_left} days left (Offline)")
                enable_download_buttons(True)
                return True
        messagebox.showerror("Error", f"Cannot start trial.\n{str(e)}")
        return False

def verify_license_online(license_key, machine_id):
    try:
        print(f"🔐 Verifying license for machine: {machine_id[:8]}...")
        clean_license = license_key.strip().replace(" ", "").replace("-", "").upper()
        if len(clean_license) != 24:
            return False, "invalid_format"
        
        resp = requests.post(f"{LICENSE_SERVER}/verify", 
                           json={"license_key": clean_license, "machine_id": machine_id}, 
                           timeout=10)
        resp.raise_for_status()
        data = resp.json()
        print(f"📄 License verification response: {data}")
        return data.get("valid", False), data.get("type", None)
    except Exception as e:
        print(f"❌ License verification error: {e}")
        return False, "connection_error"

def ask_for_license():
    machine_id = get_machine_id()
    license_key = simpledialog.askstring("License Key", "Enter your 24-character license key:")
    if license_key:
        clean_license = license_key.strip().replace(" ", "").replace("-", "").upper()
        if len(clean_license) != 24:
            messagebox.showerror("Invalid Format", "License key must be 24 characters long.")
            return ask_for_license()
            
        valid, ltype = verify_license_online(clean_license, machine_id)
        if valid:
            license_data = {
                "machine_id": machine_id, 
                "license_key": clean_license, 
                "license_type": ltype,
                "activated_at": datetime.now().isoformat(),
                "last_verified": datetime.now().isoformat()
            }
            save_local_token(license_data)
            license_status_var.set("License: Active")
            enable_download_buttons(True)
            messagebox.showinfo("License Activated", "License activated successfully! Lifetime access granted.")
            return True
        else:
            if ltype == "connection_error":
                messagebox.showerror("Connection Error", "Cannot connect to license server. Please check your internet connection.")
            else:
                messagebox.showerror("Invalid License", "License key is invalid or already used on another machine.")
    elif license_key is None:
        return False
    return False

def check_license_status():
    try:
        token = load_local_token()
        machine_id = get_machine_id()  # Now returns consistent Windows ID
        
        print(f"🔍 Checking license status for Windows Machine: {machine_id[:16]}...")

        if not token:
            print("📭 No token found, starting trial...")
            return start_trial(machine_id)

        # Check if we have stored machine ID in token
        stored_machine_id = token.get("machine_id")
        if stored_machine_id and stored_machine_id != machine_id:
            print("⚠️ Machine ID mismatch detected!")
            print(f"   Stored: {stored_machine_id[:16]}...")
            print(f"   Current: {machine_id[:16]}...")
            
            # This should rarely happen with Windows Machine GUID
            # Try to recover by showing error and asking for re-activation
            response = messagebox.askyesno(
                "Hardware Change Detected", 
                "Significant hardware changes detected.\n\n"
                "This usually happens after:\n"
                "• Motherboard replacement\n"
                "• Major hardware upgrades\n"
                "• Windows reinstallation\n\n"
                "Do you want to reactivate your license?"
            )
            if response:
                # Clear old token and start fresh
                if os.path.exists(TOKEN_FILE):
                    os.remove(TOKEN_FILE)
                return ask_for_license()
            else:
                enable_download_buttons(False)
                return False

        if "license_key" in token:
            print("📋 Found license in token, verifying...")
            license_key = token["license_key"]
            valid, ltype = verify_license_online(license_key, machine_id)
            
            if valid:
                # Update token with success
                token.update({
                    "machine_id": machine_id,
                    "last_verified": datetime.now().isoformat(),
                    "verification_status": "valid",
                    "windows_verified": True
                })
                save_local_token(token)
                license_status_var.set("License: Active ✓")
                enable_download_buttons(True)
                return True
            else:
                if ltype in ["connection_error", "timeout", "server_error"]:
                    # Offline grace period - 60 days for Windows
                    last_verified = token.get("last_verified")
                    if last_verified:
                        last_verify_date = datetime.fromisoformat(last_verified)
                        days_since_verify = (datetime.now() - last_verify_date).days
                        if days_since_verify < 60:  # 60-day offline grace
                            license_status_var.set(f"License: Active (Offline - {60-days_since_verify}d left)")
                            enable_download_buttons(True)
                            return True
                    
                    # Offline period expired or no previous verification
                    license_status_var.set("License: Verification Required")
                    enable_download_buttons(False)
                    return ask_for_license()
                    
                elif ltype == "machine_mismatch":
                    messagebox.showerror(
                        "License Activation Error", 
                        "This license is already activated on a different computer.\n\n"
                        "If this is your only computer, please contact support.\n"
                        "Otherwise, deactivate the license from the other computer first."
                    )
                    enable_download_buttons(False)
                    return False
                else:
                    enable_download_buttons(False)
                    return ask_for_license()

        elif "trial_start" in token:
            print("📋 Found trial in token, checking...")
            try:
                resp = requests.post(f"{LICENSE_SERVER}/trial", json={"machine_id": machine_id}, timeout=5)
                data = resp.json()
                if data.get("valid"):
                    days_left = data.get("days_left", TRIAL_DAYS)
                    license_status_var.set(f"Trial: {days_left} days left")
                    enable_download_buttons(True)
                    return True
                else:
                    enable_download_buttons(False)
                    return ask_for_license()
            except:
                trial_start = datetime.fromisoformat(token["trial_start"])
                elapsed = (datetime.now() - trial_start).days
                if elapsed < TRIAL_DAYS:
                    days_left = TRIAL_DAYS - elapsed
                    license_status_var.set(f"Trial: {days_left} days left (Offline)")
                    enable_download_buttons(True)
                    return True
                else:
                    enable_download_buttons(False)
                    return ask_for_license()

        print("📭 No valid token, starting fresh trial...")
        return start_trial(machine_id)
        
    except Exception as e:
        print(f"💥 Error in check_license_status: {e}")
        import traceback
        traceback.print_exc()
        # Graceful fallback - allow limited functionality
        license_status_var.set("Status: Check Failed - Limited Mode")
        enable_download_buttons(True)
        return True

def check_trial_or_license():
    def run_check():
        try:
            success = check_license_status()
            if not success:
                print("🔄 Initial check failed, retrying...")
                time.sleep(2)
                check_license_status()
        except Exception as e:
            print(f"💥 License check thread error: {e}")
            license_status_var.set("Status: Check Failed")
    
    threading.Thread(target=run_check, daemon=True).start()
    return True

def enable_download_buttons(state=True):
    for btn in [btn_add, btn_instant, btn_start, btn_pause, btn_resume, btn_clear_completed]:
        btn.config(state=tk.NORMAL if state else tk.DISABLED)

# ------------------ Update Checker ------------------
def check_for_updates(auto=False):
    try:
        resp = requests.get(UPDATE_CHECK_URL, timeout=5)
        latest_version = resp.text.strip()
        if latest_version != CURRENT_VERSION:
            if auto:
                log_text.insert(tk.END, f"Update available: {latest_version}\n")
                log_text.see(tk.END)
            else:
                if messagebox.askyesno("Update Available", f"New version {latest_version} available. Download?"):
                    import webbrowser
                    webbrowser.open(DOWNLOAD_PAGE_URL)
        elif not auto:
            messagebox.showinfo("No Update", "You are running the latest version.")
    except Exception as e:
        if not auto:
            messagebox.showwarning("Update Check Failed", f"Unable to check updates.\nError: {e}")
        log_text.insert(tk.END, f"Update check failed: {e}\n")
        log_text.see(tk.END)

def auto_update_check():
    threading.Thread(target=check_for_updates, kwargs={"auto": True}, daemon=True).start()

# ------------------ About Window ------------------
def show_about():
    about_window = tk.Toplevel(root)
    about_window.title("About SwiftHarryDM")
    about_window.geometry("500x400")
    about_window.resizable(False, False)
    about_window.configure(bg=COLORS["dark_bg"])
    about_window.transient(root)
    about_window.grab_set()
    
    # Center the window
    about_window.update_idletasks()
    x = (about_window.winfo_screenwidth() // 2) - (500 // 2)
    y = (about_window.winfo_screenheight() // 2) - (400 // 2)
    about_window.geometry(f"500x400+{x}+{y}")
    
    # Header
    header_frame = tk.Frame(about_window, bg=COLORS["primary"], height=100)
    header_frame.pack(fill="x", side="top")
    header_frame.pack_propagate(False)
    
    tk.Label(header_frame, text="🚀 SwiftHarryDM", font=("Arial", 24, "bold"), 
             bg=COLORS["primary"], fg="white").pack(expand=True)
    
    tk.Label(header_frame, text="Professional Download Manager", font=("Arial", 12), 
             bg=COLORS["primary"], fg="white").pack(expand=True)
    
    # Content
    content_frame = tk.Frame(about_window, bg=COLORS["dark_bg"], padx=20, pady=20)
    content_frame.pack(fill="both", expand=True, padx=10, pady=10)
    
    info_text = f"""
Version: {CURRENT_VERSION}

SwiftHarryDM is a powerful universal downloader that supports:
• YouTube, Vimeo, Twitter, and 1000+ sites
• Multiple formats (MP4, MP3, 1080p, 720p)
• Browser extension integration
• Queue management with pause/resume
• License and trial system

Features:
✓ High-speed downloads
✓ Format conversion
✓ Browser integration
✓ Professional UI/UX
✓ Windows support

Developed with ❤️ by Harsh Chaudhary.

© 2024 SwiftHarryDM. All rights reserved.
    """
    
    tk.Label(content_frame, text=info_text, font=("Arial", 10), 
             bg=COLORS["dark_bg"], fg=COLORS["dark_text"], justify="left", anchor="w").pack(fill="both", expand=True)
    
    # Close button
    tk.Button(about_window, text="Close", command=about_window.destroy,
              bg=COLORS["primary"], fg="white", font=("Arial", 10, "bold"),
              width=20, height=2).pack(pady=10)

# ------------------ License Windows ------------------
def show_license_info():
    try:
        with open("LICENSE.txt","r") as f:
            license_text = f.read()
        license_window = tk.Toplevel(root)
        license_window.title("License Information")
        license_window.geometry("700x500")
        license_window.configure(bg=COLORS["dark_bg"])
        
        text_frame = tk.Frame(license_window, bg=COLORS["dark_bg"])
        text_frame.pack(fill="both", expand=True, padx=10, pady=10)
        
        scrollbar = ttk.Scrollbar(text_frame, style="IDM.Vertical.TScrollbar")
        scrollbar.pack(side="right", fill="y")
        
        txt = tk.Text(text_frame, wrap="word", font=("Arial",10), yscrollcommand=scrollbar.set,
                     bg=COLORS["dark_surface"], fg=COLORS["dark_text"], insertbackground=COLORS["dark_text"])
        txt.pack(side="left", fill="both", expand=True)
        scrollbar.config(command=txt.yview)
        
        txt.insert(tk.END, license_text)
        txt.config(state="disabled")
    except:
        messagebox.showerror("Error", "LICENSE.txt not found!")

def show_license_window():
    license_win = tk.Toplevel(root)
    license_win.title("License Management")
    license_win.geometry("400x300")
    license_win.resizable(False, False)
    license_win.configure(bg=COLORS["dark_bg"])
    
    license_win.transient(root)
    license_win.grab_set()
    
    # Center the window
    license_win.update_idletasks()
    x = (license_win.winfo_screenwidth() // 2) - (400 // 2)
    y = (license_win.winfo_screenheight() // 2) - (300 // 2)
    license_win.geometry(f"400x300+{x}+{y}")
    
    # Header
    header_frame = tk.Frame(license_win, bg=COLORS["primary"], height=60)
    header_frame.pack(fill="x", side="top")
    header_frame.pack_propagate(False)
    
    tk.Label(header_frame, text="License Management", font=("Arial", 16, "bold"), 
             bg=COLORS["primary"], fg="white").pack(expand=True)
    
    # Current status frame
    status_frame = tk.Frame(license_win, padx=20, pady=20, bg=COLORS["dark_bg"])
    status_frame.pack(fill="x", padx=10, pady=10)
    
    tk.Label(status_frame, text="Current Status:", font=("Arial", 12, "bold"), 
             bg=COLORS["dark_bg"], fg=COLORS["dark_text"]).pack(anchor="w")
    current_status_label = tk.Label(status_frame, textvariable=license_status_var, 
                                   font=("Arial", 11), fg=COLORS["success"], bg=COLORS["dark_bg"])
    current_status_label.pack(anchor="w", pady=(5, 0))
    
    # Separator
    ttk.Separator(license_win, orient="horizontal").pack(fill="x", padx=20, pady=5)
    
    # Actions frame
    actions_frame = tk.Frame(license_win, padx=20, pady=10, bg=COLORS["dark_bg"])
    actions_frame.pack(fill="both", expand=True, padx=10, pady=10)
    
    tk.Label(actions_frame, text="License Actions:", font=("Arial", 12, "bold"), 
             bg=COLORS["dark_bg"], fg=COLORS["dark_text"]).pack(anchor="w")
    
    # Buttons
    btn_enter_license = tk.Button(actions_frame, text="Enter License Key", command=ask_for_license, 
                                 bg=COLORS["primary"], fg="white", width=20, font=("Arial", 10),
                                 relief="flat", padx=10, pady=5)
    btn_enter_license.pack(pady=10)
    
    btn_check_status = tk.Button(actions_frame, text="Check License Status", 
                                command=lambda: check_trial_or_license(), 
                                bg=COLORS["success"], fg="white", width=20, font=("Arial", 10),
                                relief="flat", padx=10, pady=5)
    btn_check_status.pack(pady=5)
    
    btn_view_info = tk.Button(actions_frame, text="View License Information", 
                             command=show_license_info, 
                             bg=COLORS["secondary"], fg="white", width=20, font=("Arial", 10),
                             relief="flat", padx=10, pady=5)
    btn_view_info.pack(pady=10)
    
    # Close button
    btn_close = tk.Button(license_win, text="Close", command=license_win.destroy,
                         bg=COLORS["danger"], fg="white", width=15, font=("Arial", 10, "bold"),
                         relief="flat", padx=10, pady=5)
    btn_close.pack(pady=10)

# ------------------ Universal Downloader ------------------
# ------------------ Universal Downloader ------------------
class UniversalDownloader:
    def __init__(self, url, fmt="best", save_path=None, progress_hook=None, is_extension=False):
        self.url = url
        self.fmt = fmt
        self.save_path = save_path or get_default_save_path()
        self.progress_hook = progress_hook
        self.is_extension = is_extension
        
        # Ensure save directory exists and is writable
        try:
            os.makedirs(self.save_path, exist_ok=True)
            # Test write permission
            test_file = os.path.join(self.save_path, "write_test.tmp")
            with open(test_file, 'w') as f:
                f.write("test")
            os.remove(test_file)
            print(f"✅ [DOWNLOADER] Save path is writable: {self.save_path}")
        except Exception as e:
            print(f"❌ [DOWNLOADER] Cannot write to {self.save_path}: {e}")
            # Fallback to current directory
            self.save_path = os.getcwd()
            print(f"🔄 [DOWNLOADER] Using fallback path: {self.save_path}")
        
        print(f"🔧 [DOWNLOADER] Initialized: {url} -> {fmt} -> {self.save_path}")

    def get_best_working_format(self, ydl, url, requested_format):
        """Get the best working format that avoids SABR streaming issues"""
        try:
            info = ydl.extract_info(url, download=False)
            available_formats = info.get('formats', [])
            
            # For MP3, prefer audio formats
            if requested_format.lower() == "mp3":
                audio_formats = [f for f in available_formats if f.get('vcodec') == 'none']
                if audio_formats:
                    # Get the best audio format
                    best_audio = max(audio_formats, key=lambda x: x.get('abr', 0) or x.get('quality', 0))
                    return best_audio['format_id']
            
            # For video, avoid formats that might trigger SABR
            video_formats = [f for f in available_formats if f.get('vcodec') != 'none']
            if video_formats:
                # Filter out problematic formats
                safe_formats = [f for f in video_formats if 
                              not f.get('format_note', '').lower().startswith('dash') and
                              not f.get('protocol', '').startswith('m3u8')]
                
                if safe_formats:
                    # Get the best safe format
                    best_safe = max(safe_formats, key=lambda x: x.get('height', 0) or x.get('quality', 0))
                    return best_safe['format_id']
            
            # Fallback to default
            return format_mapping(requested_format)
            
        except Exception as e:
            print(f"⚠️ [DOWNLOADER] Format selection failed, using default: {e}")
            return format_mapping(requested_format)

    def download(self):
        try:
            print(f"🔧 [DOWNLOADER] Starting download: {self.url}")
            print(f"🔧 [DOWNLOADER] Requested format: {self.fmt}")
            
            # Get ffmpeg path using your utils function
            ffmpeg_path = get_ffmpeg_path()
            ffmpeg_dir = os.path.dirname(ffmpeg_path) if ffmpeg_path and os.path.exists(ffmpeg_path) else None
            
            # Get the format selector using your utils function
            format_selector = format_mapping(self.fmt)
            print(f"🔧 [DOWNLOADER] yt-dlp format selector: {format_selector}")

            # Enhanced yt-dlp options for better YouTube compatibility
            opts = {
                "progress_hooks": [self.progress_hook] if self.progress_hook else [],
                "ignoreerrors": True,
                "retries": 5,
                "fragment_retries": 5,
                "skip_unavailable_fragments": True,
                "noplaylist": True,
                "format": format_selector,
                "nocheckcertificate": False,
                "no_warnings": False,
                "quiet": True,
                "extract_flat": False,
                "socket_timeout": 30,
                "extractor_retries": 3,
            }

            # Remove sleep delays for faster downloads
            opts["sleep_interval"] = 0
            opts["max_sleep_interval"] = 0

            # Add YouTube-specific configuration to avoid SABR streaming issues
            if "youtube.com" in self.url or "youtu.be" in self.url:
                opts.update({
                    "http_headers": {
                        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36',
                        'Accept': 'text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8',
                        'Accept-Language': 'en-us,en;q=0.5',
                        'Accept-Encoding': 'gzip,deflate',
                        'Accept-Charset': 'ISO-8859-1,utf-8;q=0.7,*;q=0.7',
                        'Connection': 'keep-alive',
                    },
                    # Use formats that work around SABR streaming
                    "format": "bestaudio/best" if self.fmt.lower() == "mp3" else "best[height<=1080]",
                    # Avoid problematic clients that trigger SABR
                    "extractor_args": {
                        "youtube": {
                            "player_client": ["android", "web"],
                            "player_skip": ["configs", "webpage"]
                        }
                    },
                })

            # Handle MP3 conversion
            if self.fmt.lower() == "mp3":
                if not ffmpeg_path or ffmpeg_path == 'ffmpeg':
                    # FFmpeg not available, fallback to best audio
                    print("⚠️ [DOWNLOADER] FFmpeg not available, downloading best audio format")
                    opts["format"] = "bestaudio/best"
                    opts["outtmpl"] = os.path.join(self.save_path, "%(title)s.%(ext)s")
                else:
                    # FFmpeg available, setup MP3 conversion
                    opts.update({
                        "postprocessors": [{
                            'key': 'FFmpegExtractAudio',
                            'preferredcodec': 'mp3',
                            'preferredquality': '192',
                        }],
                        "outtmpl": os.path.join(self.save_path, "%(title)s.%(ext)s"),
                        "ffmpeg_location": ffmpeg_dir,
                        "postprocessor_args": ['-y'],
                    })
                    print(f"🔧 [DOWNLOADER] MP3 conversion configured with FFmpeg")
            else:
                # For video formats
                filename_template = "%(title)s.%(ext)s"
                opts.update({
                    "outtmpl": os.path.join(self.save_path, filename_template),
                })
                if ffmpeg_path and ffmpeg_path != 'ffmpeg' and os.path.exists(ffmpeg_path):
                    opts["ffmpeg_location"] = ffmpeg_dir
                    opts["merge_output_format"] = "mp4"

            print(f"🔧 [DOWNLOADER] Starting download with yt-dlp...")
            
            final_path = None
            with YoutubeDL(opts) as ydl:
                print(f"🔧 [DOWNLOADER] Extracting info for: {self.url}")
                
                try:
                    # First try to get info without downloading
                    info = ydl.extract_info(self.url, download=False)
                    if not info:
                        raise Exception("Failed to extract video information")
                    
                    # For YouTube, try to select a better format to avoid SABR issues
                    if "youtube.com" in self.url or "youtu.be" in self.url:
                        try:
                            better_format = self.get_best_working_format(ydl, self.url, self.fmt)
                            if better_format != opts["format"]:
                                print(f"🔄 [DOWNLOADER] Using optimized format: {better_format}")
                                ydl.params["format"] = better_format
                        except Exception as format_error:
                            print(f"⚠️ [DOWNLOADER] Format optimization failed: {format_error}")
                    
                    print(f"🔧 [DOWNLOADER] Video info extracted successfully, starting download...")
                    
                    # Now download the video
                    ydl.download([self.url])
                    
                    # Get the expected filename
                    final_filename = ydl.prepare_filename(info)
                    final_path = os.path.abspath(final_filename)
                    
                except Exception as extract_error:
                    print(f"❌ [DOWNLOADER] Error during extraction: {extract_error}")
                    # Try with different format as fallback
                    print("🔄 [DOWNLOADER] Trying with fallback format...")
                    if "youtube.com" in self.url or "youtu.be" in self.url:
                        ydl.params["format"] = "best"
                    info = ydl.extract_info(self.url, download=True)
                    if not info:
                        raise extract_error
                    final_filename = ydl.prepare_filename(info)
                    final_path = os.path.abspath(final_filename)
            
            # Handle MP3 file verification
            if self.fmt.lower() == "mp3" and final_path:
                # yt-dlp should have already converted to .mp3 if FFmpeg was available
                mp3_path = os.path.splitext(final_path)[0] + ".mp3"
                
                if os.path.exists(mp3_path):
                    print(f"✅ [DOWNLOADER] MP3 file created: {mp3_path}")
                    final_path = mp3_path
                elif os.path.exists(final_path):
                    # If original file exists but isn't MP3, try conversion using your utils
                    if not final_path.endswith('.mp3'):
                        print(f"🔄 [DOWNLOADER] Converting to MP3 using utils: {final_path}")
                        mp3_path = os.path.splitext(final_path)[0] + ".mp3"
                        if convert_to_mp3(final_path, mp3_path):
                            final_path = mp3_path
                            print(f"✅ [DOWNLOADER] MP3 conversion successful: {final_path}")
                        else:
                            print(f"⚠️ [DOWNLOADER] MP3 conversion failed, keeping original: {final_path}")
                else:
                    # Try to find any downloaded audio file
                    base_name = os.path.splitext(os.path.basename(final_path))[0]
                    for file in os.listdir(self.save_path):
                        if file.startswith(base_name) and (file.endswith('.mp3') or file.endswith('.m4a') or file.endswith('.webm')):
                            found_path = os.path.join(self.save_path, file)
                            final_path = found_path
                            print(f"🔍 [DOWNLOADER] Found audio file: {final_path}")
                            break
            
            # Verify the final file exists
            if not final_path or not os.path.exists(final_path):
                # Look for any recently created files in the download directory
                downloaded_files = []
                try:
                    files = [f for f in os.listdir(self.save_path) 
                            if os.path.isfile(os.path.join(self.save_path, f))]
                    if files:
                        # Sort by modification time, newest first
                        files.sort(key=lambda x: os.path.getmtime(os.path.join(self.save_path, x)), reverse=True)
                        for file in files[:5]:  # Check last 5 files
                            file_path = os.path.join(self.save_path, file)
                            # Check if it was created recently (within last 2 minutes)
                            if os.path.getmtime(file_path) > time.time() - 120:
                                downloaded_files.append(file_path)
                                print(f"🔍 [DOWNLOADER] Recently created file: {file_path}")
                except Exception as e:
                    print(f"⚠️ [DOWNLOADER] Error finding files: {e}")
                
                if downloaded_files:
                    final_path = downloaded_files[0]
                    print(f"🔍 [DOWNLOADER] Using most recent file: {final_path}")
                else:
                    raise Exception(f"Download failed - no files were created. YouTube might be blocking the download.")
            
            if not os.path.exists(final_path):
                raise Exception(f"Download completed but file not found: {final_path}")
            
            file_size = os.path.getsize(final_path)
            print(f"✅ [DOWNLOADER] Download successful: {final_path} ({file_size} bytes)")
            return final_path
            
        except Exception as e:
            print(f"❌ [DOWNLOADER] Download failed: {e}")
            import traceback
            traceback.print_exc()
            
            # Provide more specific error messages
            error_msg = str(e)
            if "403" in error_msg or "Forbidden" in error_msg:
                raise Exception("YouTube is blocking the download (403 Forbidden). This could be due to:\n• Age-restricted content\n• Regional restrictions\n• YouTube API changes\n\nTry again later or use a VPN.")
            elif "Unsupported URL" in error_msg:
                raise Exception("Unsupported URL or website. Please check the URL and try again.")
            elif "FFmpeg" in error_msg:
                raise Exception("FFmpeg error during conversion. Please check FFmpeg installation.")
            elif "file not found" in error_msg.lower():
                raise Exception("Download failed - the file was not created. This usually means the download was blocked or interrupted.")
            else:
                raise Exception(f"Download failed: {error_msg}")

    def merge_video_audio(self, video_file, audio_file, output_file):
        """Manually merge video and audio streams using ffmpeg - uses your utils function"""
        try:
            # Use your existing manual_merge_video_audio function from utils
            success = manual_merge_video_audio(video_file, audio_file, output_file)
            if success:
                return output_file
            return None
        except Exception as e:
            print(f"❌ [MERGER] Merge error: {e}")
            return None

    def fallback_download(self):
        """Enhanced fallback downloader with better filename handling"""
        try:
            import urllib.parse
            from pathlib import Path
            
            # Create a safe filename from URL
            parsed_url = urllib.parse.urlparse(self.url)
            if "facebook.com" in self.url:
                # Extract video ID from Facebook URL
                video_id = None
                if "v=" in self.url:
                    video_id = self.url.split("v=")[-1].split("&")[0]
                elif "video/" in self.url:
                    video_id = self.url.split("video/")[-1].split("/")[0]
                
                if video_id and video_id.isdigit():
                    filename = f"facebook_video_{video_id}.mp4"
                else:
                    filename = "facebook_video.mp4"
            else:
                # Generic filename from URL
                path = Path(parsed_url.path)
                if path.name and path.suffix:
                    filename = path.name
                else:
                    filename = "downloaded_video.mp4"
            
            # Sanitize filename
            filename = sanitize_filename(filename)
            filepath = os.path.join(self.save_path, filename)
            
            print(f"🔧 [FALLBACK] Downloading to: {filepath}")
            
            # Use requests with stream for basic download
            headers = {
                'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36'
            }
            
            response = requests.get(self.url, headers=headers, stream=True, timeout=30)
            response.raise_for_status()
            
            total_size = int(response.headers.get('content-length', 0))
            downloaded = 0
            
            with open(filepath, 'wb') as f:
                for chunk in response.iter_content(chunk_size=8192):
                    if chunk:
                        f.write(chunk)
                        downloaded += len(chunk)
                        
                        # Update progress if hook exists
                        if self.progress_hook and total_size > 0:
                            percent = (downloaded / total_size) * 100
                            # Simulate progress update
                            fake_info = {
                                'status': 'downloading',
                                '_percent_str': f'{percent:.1f}%',
                                'total_bytes': total_size,
                                'downloaded_bytes': downloaded
                            }
                            self.progress_hook(fake_info)
            
            print(f"✅ [FALLBACK] Download completed: {filepath}")
            return filepath
            
        except Exception as fallback_error:
            print(f"❌ [FALLBACK] Fallback download also failed: {fallback_error}")
            raise fallback_error
        
# ------------- Verifying FFMPEG Installation ---------------
def verify_ffmpeg_installation():
    """Verify FFmpeg is available and working"""
    try:
        ffmpeg_path = get_ffmpeg_path()
        if not os.path.exists(ffmpeg_path):
            print("❌ FFmpeg not found! MP3 conversion will not work.")
            print("💡 Please download FFmpeg from: https://ffmpeg.org/download.html")
            print("💡 Or run the setup script to install it automatically.")
            return False
        
        # Test if FFmpeg works
        result = subprocess.run([ffmpeg_path, "-version"], capture_output=True, text=True, timeout=10)
        if result.returncode == 0:
            print("✅ FFmpeg is properly installed and working")
            return True
        else:
            print("❌ FFmpeg found but not working properly")
            return False
    except Exception as e:
        print(f"❌ Error verifying FFmpeg: {e}")
        return False
    
def get_ffmpeg_path():
    """Get FFmpeg path with multiple fallback options"""
    # First, check if FFmpeg is in PATH
    try:
        if platform.system().lower() == "windows":
            result = subprocess.run(['where', 'ffmpeg'], capture_output=True, text=True, timeout=5)
        else:
            result = subprocess.run(['which', 'ffmpeg'], capture_output=True, text=True, timeout=5)
        
        if result.returncode == 0:
            ffmpeg_path = result.stdout.strip().split('\n')[0]
            if os.path.exists(ffmpeg_path):
                print(f"✅ Found FFmpeg in PATH: {ffmpeg_path}")
                return ffmpeg_path
    except:
        pass
    
    # Check common installation locations
    common_paths = []
    
    if platform.system().lower() == "windows":
        common_paths = [
            r"C:\ffmpeg\bin\ffmpeg.exe",
            r"C:\Program Files\ffmpeg\bin\ffmpeg.exe",
            os.path.join(os.path.expanduser("~"), "ffmpeg", "bin", "ffmpeg.exe"),
            os.path.join(os.path.dirname(os.path.abspath(__file__)), "ffmpeg", "bin", "ffmpeg.exe"),
        ]
    else:
        common_paths = [
            "/usr/bin/ffmpeg",
            "/usr/local/bin/ffmpeg",
            os.path.join(os.path.expanduser("~"), "ffmpeg", "bin", "ffmpeg"),
            os.path.join(os.path.dirname(os.path.abspath(__file__)), "ffmpeg", "bin", "ffmpeg"),
        ]
    
    for path in common_paths:
        if os.path.exists(path):
            print(f"✅ Found FFmpeg at: {path}")
            return path
    
    print("❌ FFmpeg not found in common locations")
    return None

# ------------------ Queue Functions ------------------
def add_to_queue():
    url = url_entry.get().strip()
    fmt = format_var.get()
    if not url:
        messagebox.showerror("Error", "Please enter a URL!")
        return
    
    # Use reliable save path
    save_path = filedialog.askdirectory(title="Select Save Location") or get_default_save_path()
    
    # Double-check directory creation
    try:
        os.makedirs(save_path, exist_ok=True)
        # Test if we can write to the directory
        test_file = os.path.join(save_path, "test_write.tmp")
        with open(test_file, 'w') as f:
            f.write("test")
        os.remove(test_file)
    except Exception as e:
        messagebox.showerror("Error", f"Cannot write to directory:\n{save_path}\nError: {e}")
        return
    
    item = {"url": url, "fmt": fmt, "save_path": save_path, "status": "Pending", "progress": 0}
    download_queue.append(item)
    paused_flags.append(False)
    create_queue_item(len(download_queue)-1)
    url_entry.delete(0, tk.END)
    
    # Log the save path for debugging
    log_text.insert(tk.END, f"📁 Save location: {save_path}\n")
    log_text.see(tk.END)

def instant_download():
    url = url_entry.get().strip()
    fmt = format_var.get()
    if not url:
        messagebox.showerror("Error", "Please enter a URL!")
        return
    
    # Use reliable save path
    save_path = filedialog.askdirectory(title="Select Save Location") or get_default_save_path()
    
    # Double-check directory creation
    try:
        os.makedirs(save_path, exist_ok=True)
    except Exception as e:
        messagebox.showerror("Error", f"Cannot create directory:\n{save_path}\nError: {e}")
        return
    
    item = {"url": url, "fmt": fmt, "save_path": save_path, "status": "Downloading", "progress": 0}
    download_queue.append(item)
    paused_flags.append(False)
    idx = len(download_queue)-1
    create_queue_item(idx)
    
    # Log the save path
    log_text.insert(tk.END, f"📁 Instant download to: {save_path}\n")
    log_text.see(tk.END)
    
    threading.Thread(target=download_worker, args=(idx,), daemon=True).start()
    url_entry.delete(0, tk.END)

def create_queue_item(idx):
    item = download_queue[idx]
    frame = tk.Frame(queue_container, bd=1, relief="solid", padx=10, pady=8, bg=COLORS["dark_surface"])
    frame.pack(pady=4, fill="x", padx=5)
    
    # Header with title and status
    header_frame = tk.Frame(frame, bg=COLORS["dark_surface"])
    header_frame.pack(fill="x")
    
    title = item.get('title', get_filename_from_url(item['url']))
    label = tk.Label(header_frame, text=title[:80] + "..." if len(title) > 80 else title, 
                    font=("Arial", 10, "bold"), anchor="w", bg=COLORS["dark_surface"], fg=COLORS["dark_text"])
    label.pack(side="left", fill="x", expand=True)
    
    status_label = tk.Label(header_frame, text=f"{item['status']}", 
                           font=("Arial", 9, "bold"), anchor="e", bg=COLORS["dark_surface"], fg=COLORS["dark_text"])
    status_label.pack(side="right")
    
    # Progress bar
    progress = ttk.Progressbar(frame, orient="horizontal", length=650, mode="determinate", style="IDM.Horizontal.TProgressbar")
    progress['value'] = item['progress']
    progress.pack(side="top", pady=5, fill="x")
    
    # Format and actions
    footer_frame = tk.Frame(frame, bg=COLORS["dark_surface"])
    footer_frame.pack(fill="x")
    
    format_label = tk.Label(footer_frame, text=f"Format: {item['fmt']}", 
                           font=("Arial", 8), anchor="w", bg=COLORS["dark_surface"], fg=COLORS["light_text"])
    format_label.pack(side="left")
    
    if item.get('source') == 'extension':
        source_label = tk.Label(footer_frame, text="🌐 Browser", 
                               font=("Arial", 8), bg=COLORS["dark_surface"], fg=COLORS["primary"])
        source_label.pack(side="left", padx=(10, 0))
    
    queue_frames.append({"frame": frame, "label": label, "status_label": status_label, 
                        "progress": progress, "format_label": format_label})
    update_queue_item_color(idx)

def update_queue_item(idx):
    if idx >= len(download_queue) or idx >= len(queue_frames):
        return
        
    item = download_queue[idx]
    frame_data = queue_frames[idx]
    
    title = item.get('title', get_filename_from_url(item['url']))
    frame_data['label'].config(text=title[:80] + "..." if len(title) > 80 else title)
    frame_data['status_label'].config(text=f"{item['status']}")
    frame_data['progress']['value'] = item['progress']
    frame_data['format_label'].config(text=f"Format: {item['fmt']}")
    update_queue_item_color(idx)

def update_queue_item_color(idx):
    item = download_queue[idx]
    frame_data = queue_frames[idx]
    color_map = {
        "Pending": COLORS["dark_surface"], 
        "Downloading": "#1e3a5f", 
        "Paused": "#5d4037", 
        "Completed": "#2e7d32", 
        "Error": "#c62828"
    }
    text_color_map = {
        "Pending": COLORS["dark_text"],
        "Downloading": "#4fc3f7", 
        "Paused": "#ffb74d", 
        "Completed": "#81c784", 
        "Error": "#ef5350"
    }
    
    bg_color = color_map.get(item['status'], COLORS["dark_surface"])
    text_color = text_color_map.get(item['status'], COLORS["dark_text"])
    
    frame_data['frame'].config(bg=bg_color)
    frame_data['label'].config(bg=bg_color, fg=text_color)
    frame_data['status_label'].config(bg=bg_color, fg=text_color)
    frame_data['format_label'].config(bg=bg_color, fg=text_color)

def start_selected():
    for idx, item in enumerate(download_queue):
        if item["status"] == "Pending":
            item["status"] = "Downloading"
            update_queue_item(idx)
            threading.Thread(target=download_worker, args=(idx,), daemon=True).start()

def pause_selected():
    for idx, item in enumerate(download_queue):
        if item["status"] == "Downloading":
            paused_flags[idx] = True
            item["status"] = "Paused"
            update_queue_item(idx)

def resume_selected():
    for idx, item in enumerate(download_queue):
        if item["status"] == "Paused":
            paused_flags[idx] = False
            item["status"] = "Downloading"
            update_queue_item(idx)
            threading.Thread(target=download_worker, args=(idx,), daemon=True).start()

def clear_completed():
    global download_queue, paused_flags, queue_frames
    
    completed_indices = [i for i, item in enumerate(download_queue) if item["status"] == "Completed"]
    
    # Remove from the end to avoid index issues
    for i in sorted(completed_indices, reverse=True):
        if i < len(queue_frames):
            queue_frames[i]['frame'].destroy()
    
    # Rebuild the lists
    download_queue = [item for i, item in enumerate(download_queue) if i not in completed_indices]
    paused_flags = [flag for i, flag in enumerate(paused_flags) if i not in completed_indices]
    queue_frames = []
    
    # Recreate all queue items
    for i in range(len(download_queue)):
        create_queue_item(i)

def download_worker(idx):
    if idx >= len(download_queue):
        print(f"❌ [WORKER] Index {idx} out of range!")
        return
        
    item = download_queue[idx]
    url, fmt, save_path = item["url"], item["fmt"], item["save_path"]
    title = item.get("title", get_filename_from_url(url))
    
    # Check if this is from extension
    is_extension = item.get('source') == 'extension'

    print(f"🔧 [WORKER] Starting download worker for: {title}")
    print(f"🔧 [WORKER] URL: {url}")
    print(f"🔧 [WORKER] Format: {fmt}")
    print(f"🔧 [WORKER] Save path: {save_path}")
    print(f"🔧 [WORKER] From extension: {is_extension}")

    # Show download progress window
    root.after(0, lambda: show_download_window(root, item, download_queue, idx))

    # Store file size information
    total_size = 0
    downloaded_size = 0

    def progress_hook(d):
        nonlocal total_size, downloaded_size
        
        if d['status'] == 'downloading':
            # Get actual file size information from yt-dlp
            if 'total_bytes' in d and d['total_bytes']:
                total_size = d['total_bytes']
                item['total_size'] = total_size  # Store in item for download window
            
            if 'downloaded_bytes' in d and d['downloaded_bytes']:
                downloaded_size = d['downloaded_bytes']
                item['downloaded_size'] = downloaded_size
            
            # Calculate progress percentage
            if total_size > 0:
                progress_percent = (downloaded_size / total_size) * 100
            else:
                progress_percent = clean_percent(d.get('_percent_str','0%'))
            
            item['progress'] = progress_percent
            update_queue_item(idx)
            
        elif d['status'] == 'finished':
            item['progress'] = 100
            item['total_size'] = total_size  # Ensure final size is stored
            update_queue_item(idx)
            print(f"✅ [WORKER] Download finished: {title}")

    # Add retry mechanism for failed downloads
    max_retries = 3
    for attempt in range(max_retries + 1):
        try:
            if attempt > 0:
                # On retry, use a simpler format to increase success chances
                retry_fmt = "bestaudio" if fmt == "mp3" else "best"
                log_text.insert(tk.END, f"🔄 Retry {attempt} with simplified format...\n")
                log_text.see(tk.END)
                downloader = UniversalDownloader(url, retry_fmt, save_path, progress_hook, is_extension)
            else:
                downloader = UniversalDownloader(url, fmt, save_path, progress_hook, is_extension)
                
            log_text.insert(tk.END, f"🚀 Starting download (attempt {attempt + 1}): {title}\n")
            log_text.insert(tk.END, f"📁 Save location: {save_path}\n")
            log_text.see(tk.END)
            print(f"🎯 [WORKER] Calling UniversalDownloader for: {title}")
            
            downloader.download()
            item["status"] = "Completed"
            update_queue_item(idx)
            
            success_msg = f"✅ Download completed: {title}\n"
            log_text.insert(tk.END, success_msg)
            log_text.see(tk.END)
            print(f"🎉 [WORKER] Download successful: {title}")
            
            # Close download window after completion
            root.after(100, lambda: close_download_window(idx))
            break  # Success, exit retry loop
            
        except Exception as e:
            if attempt < max_retries:
                wait_time = (attempt + 1) * 3  # 3, 6, 9 seconds (shorter waits)
                retry_msg = f"🔄 Retry {attempt + 1}/{max_retries} in {wait_time} seconds...\n"
                log_text.insert(tk.END, retry_msg)
                log_text.see(tk.END)
                print(f"🔄 [WORKER] Retry {attempt + 1}/{max_retries} in {wait_time}s")
                time.sleep(wait_time)
            else:
                # Final failure
                item["status"] = "Error"
                update_queue_item(idx)
                error_msg = f"❌ Download failed after {max_retries + 1} attempts: {title} - {str(e)}\n"
                log_text.insert(tk.END, error_msg)
                log_text.see(tk.END)
                print(f"💥 [WORKER] Download failed: {title} - Error: {str(e)}")
                
                # Close download window on error
                root.after(100, lambda: close_download_window(idx))
                
# ------------------ Professional IDM GUI ------------------
root = tk.Tk()
root.title(f"SwiftHarryDM v{CURRENT_VERSION} - Professional Download Manager")
root.geometry("1200x700")
root.configure(bg=COLORS["dark_bg"])
root.minsize(1000, 600)

# Configure IDM styles
configure_idm_styles()

# Center window on screen
root.update_idletasks()
width = 1200
height = 700
x = (root.winfo_screenwidth() // 2) - (width // 2)
y = (root.winfo_screenheight() // 2) - (height // 2)
root.geometry(f"{width}x{height}+{x}+{y}")

# ------------------ Modern IDM Menu Bar ------------------
menu_bar = tk.Menu(root, bg=COLORS["dark_bg"], fg=COLORS["dark_text"], 
                  activebackground=COLORS["primary"], activeforeground=COLORS["dark_text"],
                  relief="flat", bd=0, font=('Arial', 10))

# File Menu
file_menu = tk.Menu(menu_bar, tearoff=0, bg=COLORS["dark_surface"], fg=COLORS["dark_text"],
                   activebackground=COLORS["primary"], activeforeground=COLORS["dark_text"])
file_menu.add_command(label="Add Download", command=add_to_queue, accelerator="Ctrl+N")
file_menu.add_command(label="Instant Download", command=instant_download, accelerator="Ctrl+I")
file_menu.add_separator()
file_menu.add_command(label="Import List...")
file_menu.add_command(label="Export List...")
file_menu.add_separator()
file_menu.add_command(label="Exit", command=root.quit, accelerator="Alt+F4")
menu_bar.add_cascade(label="File", menu=file_menu)

# Download Menu
download_menu = tk.Menu(menu_bar, tearoff=0, bg=COLORS["dark_surface"], fg=COLORS["dark_text"],
                       activebackground=COLORS["primary"], activeforeground=COLORS["dark_text"])
download_menu.add_command(label="Start Selected", command=start_selected, accelerator="F5")
download_menu.add_command(label="Pause Selected", command=pause_selected, accelerator="F6")
download_menu.add_command(label="Resume Selected", command=resume_selected, accelerator="F7")
download_menu.add_separator()
download_menu.add_command(label="Schedule Downloads...")
download_menu.add_command(label="Category...")
menu_bar.add_cascade(label="Download", menu=download_menu)

# View Menu
view_menu = tk.Menu(menu_bar, tearoff=0, bg=COLORS["dark_surface"], fg=COLORS["dark_text"],
                   activebackground=COLORS["primary"], activeforeground=COLORS["dark_text"])
view_menu.add_checkbutton(label="Toolbar")
view_menu.add_checkbutton(label="Status Bar")
view_menu.add_separator()
view_menu.add_command(label="Language")
view_menu.add_command(label="Skin")
menu_bar.add_cascade(label="View", menu=view_menu)

# License Menu
license_menu = tk.Menu(menu_bar, tearoff=0, bg=COLORS["dark_surface"], fg=COLORS["dark_text"],
                      activebackground=COLORS["primary"], activeforeground=COLORS["dark_text"])
license_menu.add_command(label="License Management", command=show_license_window)
license_menu.add_command(label="Enter License Key", command=ask_for_license)
license_menu.add_command(label="Check License Status", command=lambda: check_trial_or_license())
menu_bar.add_cascade(label="License", menu=license_menu)

# Help Menu
help_menu = tk.Menu(menu_bar, tearoff=0, bg=COLORS["dark_surface"], fg=COLORS["dark_text"],
                   activebackground=COLORS["primary"], activeforeground=COLORS["dark_text"])
help_menu.add_command(label="Documentation")
help_menu.add_command(label="Check for Updates", command=lambda: check_for_updates(auto=False))
help_menu.add_separator()
help_menu.add_command(label="About SwiftHarryDM", command=show_about)
menu_bar.add_cascade(label="Help", menu=help_menu)

root.config(menu=menu_bar)

# ------------------ Header Frame ------------------
header_frame = tk.Frame(root, bg=COLORS["header_bg"], height=80)
header_frame.pack(fill="x", side="top")
header_frame.pack_propagate(False)

# Header Content
header_content = tk.Frame(header_frame, bg=COLORS["header_bg"])
header_content.pack(expand=True, fill="both", padx=20)

# Logo and title
title_frame = tk.Frame(header_content, bg=COLORS["header_bg"])
title_frame.pack(side="left")

# Logo placeholder
logo_label = tk.Label(title_frame, text="🚀", font=("Arial", 24), 
                     bg=COLORS["header_bg"], fg="white")
logo_label.pack(side="left", padx=(0, 10))

title_label = tk.Label(title_frame, text="SwiftHarryDM", font=("Arial", 20, "bold"), 
                      bg=COLORS["header_bg"], fg="white")
title_label.pack(side="left")

version_label = tk.Label(title_frame, text=f"v{CURRENT_VERSION}", font=("Arial", 10), 
                        bg=COLORS["header_bg"], fg="#CCCCCC")
version_label.pack(side="left", padx=(5, 0))

# Status indicators
status_frame = tk.Frame(header_content, bg=COLORS["header_bg"])
status_frame.pack(side="right")

license_status_var = tk.StringVar(value="Checking license...")
license_label = tk.Label(status_frame, textvariable=license_status_var, 
                        font=("Arial", 10, "bold"), bg=COLORS["header_bg"], fg="#4CAF50")
license_label.pack(side="top", anchor="e")

extension_status_var = tk.StringVar(value="🔌 Extension: Connecting...")
extension_label = tk.Label(status_frame, textvariable=extension_status_var, 
                          font=("Arial", 9), bg=COLORS["header_bg"], fg="#CCCCCC")
extension_label.pack(side="top", anchor="e")

# ------------------ Main Content Frame ------------------
main_frame = ttk.Frame(root, style="IDM.TFrame")
main_frame.pack(fill="both", expand=True, padx=10, pady=10)

# ------------------ Quick Download Panel ------------------
download_panel = ttk.Frame(main_frame, style="IDM.TFrame")
download_panel.pack(fill="x", pady=(0, 15))

# URL input
url_frame = ttk.Frame(download_panel, style="IDM.TFrame")
url_frame.pack(fill="x", pady=5)

tk.Label(url_frame, text="URL:", font=("Arial", 10, "bold"), 
        bg=COLORS["dark_bg"], fg=COLORS["dark_text"]).pack(side="left", padx=(0, 10))

url_entry = tk.Entry(url_frame, width=70, font=("Arial", 11), 
                    bg=COLORS["dark_surface"], fg=COLORS["dark_text"],
                    insertbackground=COLORS["dark_text"],
                    relief="solid", bd=1)
url_entry.pack(side="left", fill="x", expand=True, padx=(0, 10))

# Format selection
format_frame = ttk.Frame(download_panel, style="IDM.TFrame")
format_frame.pack(fill="x", pady=5)

tk.Label(format_frame, text="Format:", font=("Arial", 10, "bold"), 
        bg=COLORS["dark_bg"], fg=COLORS["dark_text"]).pack(side="left", padx=(0, 10))

format_var = tk.StringVar(value="best")
format_combo = ttk.Combobox(format_frame, textvariable=format_var,
                           values=["best", "1080", "720", "480", "mp3", "mp4"], 
                           state="readonly", width=15, style="IDM.TCombobox")
format_combo.pack(side="left", padx=(0, 20))

# Action buttons
button_frame = ttk.Frame(download_panel, style="IDM.TFrame")
button_frame.pack(fill="x", pady=10)

def create_idm_button(parent, text, command, color, width=15):
    return tk.Button(parent, text=text, command=command,
                    bg=color, fg="white", width=width, font=("Arial", 9, "bold"),
                    relief="flat", padx=15, pady=6, bd=0,
                    activebackground=color, activeforeground="white")

btn_add = create_idm_button(button_frame, "Add to Queue", add_to_queue, COLORS["primary"])
btn_add.pack(side="left", padx=5)

btn_instant = create_idm_button(button_frame, "Instant Download", instant_download, COLORS["success"])
btn_instant.pack(side="left", padx=5)

btn_start = create_idm_button(button_frame, "Start All", start_selected, COLORS["primary_light"])
btn_start.pack(side="left", padx=5)

btn_pause = create_idm_button(button_frame, "Pause All", pause_selected, COLORS["warning"])
btn_pause.pack(side="left", padx=5)

btn_resume = create_idm_button(button_frame, "Resume All", resume_selected, COLORS["secondary"])
btn_resume.pack(side="left", padx=5)

btn_clear_completed = create_idm_button(button_frame, "Clear Completed", clear_completed, COLORS["danger"])
btn_clear_completed.pack(side="left", padx=5)

btn_open_folder = create_idm_button(button_frame, "Open Download Folder", open_download_folder, COLORS["primary_light"])
btn_open_folder.pack(side="left", padx=5)

# ------------------ Download Queue Section ------------------
queue_section = ttk.Frame(main_frame, style="IDM.TFrame")
queue_section.pack(fill="both", expand=True, pady=(10, 0))

# Section header
header_frame = ttk.Frame(queue_section, style="IDM.TFrame")
header_frame.pack(fill="x", pady=(0, 10))

tk.Label(header_frame, text="Download Queue", font=("Arial", 14, "bold"), 
        bg=COLORS["dark_bg"], fg=COLORS["dark_text"]).pack(side="left")

queue_count_var = tk.StringVar(value="Downloads: 0")
queue_count_label = tk.Label(header_frame, textvariable=queue_count_var, font=("Arial", 11), 
                            bg=COLORS["dark_bg"], fg=COLORS["light_text"])
queue_count_label.pack(side="left", padx=(10, 0))

# Queue container with scroll
queue_container_frame = ttk.Frame(queue_section, style="IDM.TFrame")
queue_container_frame.pack(fill="both", expand=True, pady=(5, 0))

queue_canvas = tk.Canvas(queue_container_frame, bg=COLORS["dark_bg"], highlightthickness=0)
queue_scrollbar = ttk.Scrollbar(queue_container_frame, orient="vertical", command=queue_canvas.yview, style="IDM.Vertical.TScrollbar")
queue_container = tk.Frame(queue_canvas, bg=COLORS["dark_bg"])

queue_container.bind("<Configure>", lambda e: queue_canvas.configure(scrollregion=queue_canvas.bbox("all")))
queue_canvas.create_window((0,0), window=queue_container, anchor="nw", width=queue_canvas.winfo_width())
queue_canvas.configure(yscrollcommand=queue_scrollbar.set)

queue_canvas.pack(side="left", fill="both", expand=True)
queue_scrollbar.pack(side="right", fill="y")

def update_queue_display(event):
    queue_canvas.configure(scrollregion=queue_canvas.bbox("all"))
    queue_canvas.itemconfig('all', width=queue_canvas.winfo_width())

queue_canvas.bind('<Configure>', update_queue_display)

# ------------------ Log Section ------------------
log_section = ttk.Frame(main_frame, style="IDM.TFrame")
log_section.pack(fill="x", pady=(15, 0))

tk.Label(log_section, text="Activity Log:", font=("Arial", 12, "bold"), 
        bg=COLORS["dark_bg"], fg=COLORS["dark_text"]).pack(anchor="w")

log_frame = ttk.Frame(log_section, style="IDM.TFrame")
log_frame.pack(fill="x", pady=(5, 0))

log_text = tk.Text(log_frame, height=6, wrap="word", font=("Consolas", 9), 
                  relief="solid", bd=1, bg="#1e1e1e", fg="#00ff00",
                  insertbackground="#00ff00")
log_text.pack(side="left", fill="both", expand=True)

log_scrollbar = ttk.Scrollbar(log_frame, orient="vertical", command=log_text.yview, style="IDM.Vertical.TScrollbar")
log_text.configure(yscrollcommand=log_scrollbar.set)
log_scrollbar.pack(side="right", fill="y")

# ------------------ Status Bar ------------------
status_bar = tk.Frame(root, bg=COLORS["primary_dark"], height=25)
status_bar.pack(fill="x", side="bottom")
status_bar.pack_propagate(False)

# Left side - general status
left_status = tk.Label(status_bar, text="Ready", font=("Arial", 9), 
                      bg=COLORS["primary_dark"], fg="white")
left_status.pack(side="left", padx=10)

# Right side - transfer info
right_status = tk.Label(status_bar, text="Total: 0 | Active: 0 | Speed: 0 KB/s", 
                       font=("Arial", 9), bg=COLORS["primary_dark"], fg="white")
right_status.pack(side="right", padx=10)

# ------------------ Startup Functions ------------------
def update_queue_count():
    total = len(download_queue)
    active = len([item for item in download_queue if item["status"] == "Downloading"])
    queue_count_var.set(f"Downloads: {total} (Active: {active})")
    root.after(1000, update_queue_count)

def check_extension_connection():
    try:
        response = requests.get("http://127.0.0.1:5001/health", timeout=2)
        if response.status_code == 200:
            extension_status_var.set("🔌 Extension: Connected ✓")
        else:
            extension_status_var.set("🔌 Extension: Port busy")
    except:
        extension_status_var.set("🔌 Extension: Not connected")
    
    # Check again after 5 seconds
    root.after(5000, check_extension_connection)

# ------------------ Initialize App ------------------
auto_update_check()
root.after(3000, check_extension_connection)
root.after(1000, update_queue_count)

# Check trial/license at startup
if not check_trial_or_license():
    root.destroy()
    exit()

# Start the main loop
root.mainloop()