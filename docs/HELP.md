# SwiftHarryDM help

## Starting downloads

- **From the browser.** With the extension connected, click a download link as usual:
  SwiftHarryDM takes it over and shows **Download file info**. Hold **Alt** while clicking to
  let the browser download it instead. File types and the sites never to take over are set in
  **Options › File types**.
- **Add URL** (Ctrl+N), or paste or drag a link onto the main window or the floating drop
  target (**Tasks › Show drop target**).
- **Copied links.** With **Offer to download links I copy** on (Options › General), copying a
  link to a file opens **Download file info**.
- **Many links at once.** Right-click a web page and choose **Download all links**, or use
  **Tasks › Add batch download** for numbered files like `photo001.jpg … photo120.jpg`.
- **Command line:** `swiftharrydm.exe /d <url> [/p <folder>] [/f <file name>] [/n] [/a]`
  (`/n` starts without asking, `/a` adds to the main queue).

## Download file info

The file starts downloading while this window is open, so a small file is often finished
before you press **Start download**. Change the category, folder, name or description here;
they are applied when you press **Start download** (the data already downloaded moves along).
**Download later** keeps the file in the chosen queue, **Cancel** removes it.

- **Remember this folder for the category** makes the folder the category's default.
- **Preview the files in this ZIP** lists an archive's contents without downloading it.
- If the address is already in your list, you can add it again with a numbered name, replace
  the existing file, or use the existing download.
- If a site asks for a user name and password, a login window opens; the login is saved,
  encrypted, in **Options › Site logins**.

## While downloading

Double-click a download to see its progress: every connection's range and speed, a speed
limit for this download, and what to do when it finishes (exit, sleep, hibernate or shut down,
with a 30-second warning). Downloads resume after pauses, lost connections, restarts and
crashes. When a link has expired, right-click the download and choose **Refresh download
address** with a new link; the part already downloaded is kept.

## Scheduler and queues

Open **Downloads › Scheduler**. Each queue downloads a set number of files at the same time.

- **Start this queue automatically** at a time, on chosen days, and stop it at a time.
- **When the queue finishes:** exit, sleep, hibernate or shut down the computer.
- **Files in the queue:** the order of the files, which you can change.

SwiftHarryDM must be running (it can wait in the notification area) for a queue to start on
time. The Q column shows each download's queue; move downloads between queues from their
right-click menu.

## Speed limiter and quota

**Downloads › Speed limiter** turns the overall limit on and off; the limit is shared fairly
between downloads. **Options › Connection** sets the limit, the connections per file and per
server, and a download quota ("no more than X MB every Y hours").

## Categories and folders

Each category has file types and a folder below your Downloads folder (Video, Music,
Documents, …). Change them in **Options › Save to**.

## Moving your list

**Tasks › Export** writes an IDM export file (`.ef2`) or a text file of the download queue, the
selected files or all files. **Tasks › Import** reads IDM export files, text files and web
pages. IDM and SwiftHarryDM read each other's export files.

## Keyboard

| Keys | Action |
|---|---|
| Ctrl+N | Add a download |
| Ctrl+F, F3 | Find, find next |
| Enter | Open the file, or show progress |
| Del | Delete from the list |
| Ctrl+A | Select all |

## The look

**View** hides the categories, arranges the list, customizes the toolbar and the list
columns, shows or hides the notification-area icon, and sets the theme and font.
