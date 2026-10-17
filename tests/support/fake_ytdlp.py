"""Scriptable stand-in for yt-dlp (process lifecycle tests). Mode via FAKE_YTDLP_MODE:

  hang      print progress forever and keep a grandchild process ("ffmpeg") alive
  success   write a file into the --paths home dir and print the SDMF line
  noresult  exit 0 without reporting a file
  age       print an age-restriction error and exit 1
Probe calls (-J) print FAKE_YTDLP_JSON. Child PIDs are written to FAKE_YTDLP_PIDS.
"""
import json
import os
import subprocess
import sys
import time

args = sys.argv[1:]
mode = os.environ.get("FAKE_YTDLP_MODE", "success")
pids = os.environ.get("FAKE_YTDLP_PIDS")

if "-J" in args:
    print(os.environ.get("FAKE_YTDLP_JSON", json.dumps({"id": "x", "title": "Fake", "format_id": "18", "ext": "mp4"})))
    sys.exit(0)

home = next(a.split("home:", 1)[1] for a in args if a.startswith("home:"))
if mode == "hang":
    child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(600)"])
    if pids:
        with open(pids, "w") as f:
            f.write(f"{os.getpid()} {child.pid}")
    n = 0
    open(os.path.join(home, "Fake.mp4.part"), "ab").write(b"p" * 1000)
    while True:
        n += 1000
        print(f"SDMP 18|downloading|{n}|1000000|NA|50000|10", flush=True)
        time.sleep(0.1)
elif mode == "success":
    path = os.path.join(home, "Fake.mp4")
    with open(path, "wb") as f:
        f.write(b"\x00" * 4096)
    print("SDMP 18|finished|4096|4096|NA|NA|NA", flush=True)
    print("SDMF " + json.dumps(path), flush=True)
elif mode == "noresult":
    print("SDMP 18|finished|4096|4096|NA|NA|NA", flush=True)
elif mode == "age":
    print("ERROR: [youtube] x: Sign in to confirm your age. This video may be inappropriate for some users.",
          file=sys.stderr, flush=True)
    sys.exit(1)
