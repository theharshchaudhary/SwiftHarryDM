# ODM components

The system is a native-feeling Windows desktop app: dense (13px base, 22px rows),
small radii, no motion except progress. Tokens: `tokens.json` (light), `tokens.dark.json`
(dark). Live CSS: `ui/src/styles.css`. Icons: ODM's own 24×24 stroke set in
`ui/src/icons.tsx` (never IDM's toolbar art). Font: Segoe UI Variable / Segoe UI (ships
with Windows), Cascadia Mono for addresses.

Keyboard rules for every window: Tab order follows reading order; Enter is the default
button; Esc is Cancel/Close; every action is reachable without a mouse; focus is visible
(2px `accent-hover` ring). Screen readers: real `<button>`, `<input>`, `role="dialog"`,
`role="menu"`/`menuitem`, `role="tablist"`/`tab`, `role="grid"` for lists.

```
Window / Dialog
  variants  modal (Modal in components/Dialog.tsx), modeless window, tool window (drop target)
  states    focused, background, busy (Loading)
  tokens    bg bg, border border-input, radius md (in-page modal), shadow pop
  a11y      role=dialog + aria-label = title; focus first field on open; Esc closes;
            focus returns to the opener
  used on   S02–S24, S30–S32

Button
  variants  default, primary, danger; sizes md 30px, sm 26px; min width 84px (md)
  states    default, hover (surface-2), active (surface-3), focus-visible, disabled (50%)
  tokens    bg surface / accent (primary), text text / on-accent, border border-input, radius sm
  a11y      <button>; primary = Enter; label stays during busy
  used on   all dialogs

Menu bar + menus (MenuBar, MenuList)
  variants  top-level, submenu, checked, radio, separator, shortcut text
  states    closed, open, item hover/focus, disabled
  tokens    bg surface, hover surface-3, text text, disabled text-muted, shadow pop
  a11y      Alt focuses the bar; arrows move; Enter/Space activate; Esc closes one level;
            mnemonics (underlined letter) on top-level items
  used on   S01, S28 (tray menu is native)

Context menu (ContextMenu)
  same as menus; opens at pointer or at the focused row with Shift+F10 / Menu key
  used on   S01 list, tree, header, toolbar

Toolbar button
  variants  large (icon over label, 56px bar), small (icon only + tooltip, 36px bar);
            split button with drop-down (Start queue / Stop queue)
  states    enabled, disabled (selection does not allow it), pressed
  tokens    text text, hover surface-2, icon currentColor
  a11y      <button> with aria-label when icon-only; role=toolbar, arrow keys between buttons
  used on   S01

Categories tree
  variants  category node, queue node (with Q icon), grabber project node
  states    collapsed, expanded, selected, drop target (accent-soft), empty
  tokens    bg surface, selected sel / on-sel, width 200px (resizable)
  a11y      role=tree / treeitem, arrows expand/collapse, type-ahead
  used on   S01

Download list (virtualized table)
  variants  main list, queue files (S11), links (S14), grabber files (S23)
  columns   file icon + name, Q, size, status, time left, rate, last try, description,
            save to, address, referer; resizable, reorderable, sortable
  states    empty ("No downloads yet" + how to add), loading, filled, selected, multi-select,
            row by job state: downloading (bar in status), paused, error (danger text),
            complete (success icon), queued (Q icon), scheduled (clock icon)
  tokens    row 22px, row-alt stripes, selection sel / on-sel, header surface-2
  a11y      role=grid, arrows + Shift/Ctrl selection, Enter = open/properties,
            Del = delete, Ctrl+A, aria-sort on headers
  used on   S01, S11, S14, S23

Segment progress bar
  one track (bar-track) with one block per connection (bar), finished parts bar-done;
  states    downloading, paused (muted), error (danger edge), complete
  a11y      role=progressbar with aria-valuenow (overall %), segments are decorative
  used on   S04

Progress bar
  variants  determinate, indeterminate (size unknown)
  tokens    bar / bar-track, bar-done when complete; height 8px (inline 6px)
  used on   S01 status cell, S04, S27, S30

Tabs (Tabs)
  variants  horizontal (S04, S11), vertical (S07 Options)
  states    active (accent underline / left border), hover, hidden (S04 hide-tab buttons)
  a11y      role=tablist/tab, aria-selected, arrows switch
  used on   S04, S07, S11

Text field / URL / password / number (Num) / time
  height 26px, padding 4px 7px, radius xs, border border-input, focus 2px accent-hover
  states    default, focus, invalid (danger border + message below), disabled (55%)
  a11y      <label for>; invalid sets aria-invalid + aria-describedby to the message
  used on   S02, S03, S07, S10, S11, S13, S17, S18, S22

Folder / file picker field
  text field + "…" button opening the native dialog; validates the folder exists
  used on   S03, S07, S10, S22

Checkbox, radio group (Check)
  native inputs + label; groups in <fieldset><legend>
  used on   everywhere

Select
  native <select>; category and queue pickers list their icons in text only
  used on   S03, S07, S11

Editable table
  table + New / Edit / Delete buttons on the right; double-click = Edit
  states    empty (one line saying what goes here), filled, selected
  used on   S07 file types exceptions, S09 server limits, site logins

Ordered list editor
  two lists (available / shown) or one list with Up / Down / Remove
  used on   S19 columns, S20 toolbar, S11 files in the queue

Time and days picker
  time input HH:MM + seven day toggles (Mon–Sun, locale order) + "every day"
  used on   S11, S24

Countdown dialog
  big seconds count, what will happen ("Turning off the computer"), primary "Do it now",
  default focus on Cancel; any key or click cancels
  used on   S12

Browser overlay panel (extension)
  variants  full (button + format list), mini (icon only)
  states    detecting, formats ready, protected (cannot download, says why), error
  tokens    its own copy of the tokens in extension CSS; follows the page's colour scheme
  a11y      reachable with Tab, not covering the player's controls
  used on   S25, S26

Toast / tray balloon
  Windows notifications for complete, failed, update available, queue done
  used on   system

Status bar
  left: selection count / total; right: global speed, speed limiter state, quota
  used on   S01

Banner (topbanner)
  variants  info (accent-soft), warning (warning tint)
  used on   S01 (extension not connected, video components missing, admin warning)
```

## Primitives in code

Already built in `ui/src/components/`: `Dialog`, `Modal`, `Tabs`, `Check`, `Num`,
`Loading`, `MenuBar`, `MenuList`, `ContextMenu`; buttons, inputs, list and bars as CSS
classes in `styles.css`. Still to build as shared primitives during `/replica-build`:
editable table, ordered list editor, time/days picker, countdown dialog, tree with
keyboard navigation.

The `/design` page the skill asks for is not worth it in a Tauri app with one page per
window kind; the check is `scripts/screenshots.mjs`, which renders every window in light
and dark.
