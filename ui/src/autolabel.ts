// Two accessibility rules applied to every window, including content added later.
//
// Screen readers need a name for every form field. Our forms put the text before the field
// ("<span>Start at</span><input>" in a .grid2 or .row), which looks right but is not
// linked. This names any field that has no name of its own after the text in front of it,
// for every window, including fields added later.

const FIELDS = "input:not([type=hidden]), select, textarea";

function hasName(el: Element): boolean {
  if (el.hasAttribute("aria-label") || el.hasAttribute("aria-labelledby") || el.hasAttribute("title")) return true;
  if (el.closest("label")) return true;
  const id = el.getAttribute("id");
  return !!id && !!document.querySelector(`label[for="${CSS.escape(id)}"]`);
}

/** Text of an element that is only text (a label cell), not another control or a group. */
function labelText(el: Element | null): string {
  if (!el || el.matches(FIELDS) || el.querySelector(FIELDS) || el.matches("button")) return "";
  return (el.textContent ?? "").replace(/\s+/g, " ").trim().slice(0, 120);
}

function nameFor(el: Element): string {
  // "<span>Size from</span><input>": the text right before the field.
  let t = labelText(el.previousElementSibling);
  if (t) return t;
  // A grid cell holding the field (".grid2 > div.row > input"): the cell before it.
  const cell = el.parentElement;
  if (cell && cell.parentElement?.classList.contains("grid2")) {
    t = labelText(cell.previousElementSibling);
    if (t) return t;
  }
  // A field followed by its unit ("[5] files at the same time"): the text after it.
  t = labelText(el.nextElementSibling);
  if (t) return t;
  // A checkbox or field in a table row: the row's text.
  const row = el.closest("tr");
  if (row) return (row.textContent ?? "").replace(/\s+/g, " ").trim().slice(0, 120);
  return el.getAttribute("placeholder") ?? "";
}

function label(root: ParentNode) {
  root.querySelectorAll(FIELDS).forEach((el) => {
    if (hasName(el)) return;
    const n = nameFor(el);
    if (n) el.setAttribute("aria-label", n);
  });
  // A scrolling table must be reachable with the keyboard, so it can be scrolled with the
  // arrow keys.
  root.querySelectorAll(".tbl:not([tabindex])").forEach((el) => el.setAttribute("tabindex", "0"));
}

export function autoLabel() {
  label(document);
  let queued = false;
  new MutationObserver(() => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => {
      queued = false;
      label(document);
    });
  }).observe(document.body, { childList: true, subtree: true });
}
