"use strict";

const { invoke } = window.__TAURI__.core;
const dialog = window.__TAURI__.dialog;

const state = { template: null, inputs: [], output: null };

const el = (id) => document.getElementById(id);
const rplFilter = { name: "REAPER preset library", extensions: ["rpl", "RPL"] };

function renderInputs() {
  const list = el("inputList");
  list.textContent = "";
  for (const path of state.inputs) {
    const li = document.createElement("li");
    li.textContent = path;
    list.appendChild(li);
  }
  el("inputsHeading").textContent =
    state.inputs.length === 0
      ? "Chosen files and folders: none yet"
      : `Chosen files and folders: ${state.inputs.length}`;
  scanInputs();
}

const SUBFOLDER_NOTICE =
  "Subfolders found. A new choice about folder handling is available below the file list.";

async function scanInputs() {
  const modeFieldset = el("folderMode");
  const notice = el("folderNotice");
  if (state.inputs.length === 0) {
    modeFieldset.hidden = true;
    notice.textContent = "";
    return;
  }
  try {
    const info = await invoke("scan_inputs", { inputs: state.inputs });
    el("inputsHeading").textContent =
      `Chosen files and folders: ${state.inputs.length} (${info.preset_files} preset files found)`;
    const wasHidden = modeFieldset.hidden;
    modeFieldset.hidden = !info.has_subfolders;
    if (info.has_subfolders && wasHidden) {
      notice.textContent = SUBFOLDER_NOTICE;
    } else if (!info.has_subfolders && notice.textContent === SUBFOLDER_NOTICE) {
      notice.textContent = "";
    }
  } catch (e) {
    notice.textContent = `Could not scan the chosen files: ${e}`;
  }
}

function suggestOutput(templatePath) {
  const sep = templatePath.includes("\\") ? "\\" : "/";
  const dir = templatePath.slice(0, templatePath.lastIndexOf(sep));
  state.output = `${dir}${sep}Converted presets.RPL`;
  el("outputPath").value = state.output;
}

function templateDir() {
  const sep = state.template.includes("\\") ? "\\" : "/";
  return state.template.slice(0, state.template.lastIndexOf(sep) + 1);
}

// Derive the output path from the template's folder plus the typed file
// name. Returns a problem description, or null when state.output is set.
function computeOutput() {
  state.output = null;
  el("outputPath").value = "";
  if (!state.template) {
    return "Choose a template first (step 1) so the output can be saved beside it.";
  }
  let name = el("outputName").value.trim();
  if (!name) {
    return "Enter a file name for the output.";
  }
  if (/[\\/:*?"<>|]/.test(name)) {
    return 'The file name cannot contain any of these characters: \\ / : * ? " < > |';
  }
  if (!/\.rpl$/i.test(name)) name += ".RPL";
  state.output = templateDir() + name;
  el("outputPath").value = state.output;
  return null;
}

function onSameFolderToggle() {
  const checked = el("sameFolder").checked;
  el("nameRow").hidden = !checked;
  el("pickOutput").hidden = checked;
  if (checked) {
    el("outputNameHint").textContent = computeOutput() ?? "";
    el("outputName").focus();
  } else {
    el("outputNameHint").textContent = "";
  }
}

function onOutputNameInput() {
  el("outputNameHint").textContent = computeOutput() ?? "";
}

async function pickTemplate() {
  const chosen = await dialog.open({ title: "Choose template RPL", filters: [rplFilter] });
  if (!chosen) return;
  try {
    const library = await invoke("inspect_template", { template: chosen });
    state.template = chosen;
    el("templatePath").value = chosen;
    el("libraryName").textContent = `Template loaded. Presets will be created for ${library}.`;
    if (el("sameFolder").checked) {
      el("outputNameHint").textContent = computeOutput() ?? "";
    } else if (!state.output) {
      suggestOutput(chosen);
    }
  } catch (e) {
    state.template = null;
    el("templatePath").value = "";
    el("libraryName").textContent = `That file did not work as a template: ${e}`;
  }
}

async function addFiles() {
  const chosen = await dialog.open({ multiple: true, title: "Choose preset files to convert" });
  if (!chosen) return;
  for (const path of chosen) {
    if (!state.inputs.includes(path)) state.inputs.push(path);
  }
  renderInputs();
}

async function addFolder() {
  const chosen = await dialog.open({ directory: true, title: "Choose a folder of presets" });
  if (!chosen) return;
  if (!state.inputs.includes(chosen)) state.inputs.push(chosen);
  renderInputs();
}

function clearInputs() {
  state.inputs = [];
  renderInputs();
}

async function scanPlugin() {
  const notice = el("folderNotice");
  if (!state.template) {
    notice.textContent =
      "Choose a template first (step 1); the scan uses it to know what to look for.";
    return;
  }
  const chosen = await dialog.open({
    title: "Choose the plug-in file to scan",
    filters: [{ name: "Plug-in binaries", extensions: ["vst3", "dll", "clap", "component", "so"] }],
  });
  if (!chosen) return;
  notice.textContent = "Scanning the plug-in; large files can take a few seconds…";
  try {
    const info = await invoke("scan_plugin", { template: state.template, plugin: chosen });
    if (!state.inputs.includes(info.folder)) state.inputs.push(info.folder);
    renderInputs();
    notice.textContent =
      `Found ${info.found} embedded preset(s) and added them to the list with numbered names. ` +
      "Rename favorites later in the library editor.";
  } catch (e) {
    notice.textContent = `Scan came up empty: ${e}`;
  }
}

async function pickOutput() {
  const chosen = await dialog.save({
    title: "Save converted RPL as",
    defaultPath: state.output || "Converted presets.RPL",
    filters: [rplFilter],
  });
  if (!chosen) return;
  state.output = chosen;
  el("outputPath").value = chosen;
}

function showResults(statusText, detailItems) {
  el("results").hidden = false;
  el("editResult").hidden = true;
  el("status").textContent = statusText;
  const list = el("detailList");
  list.textContent = "";
  for (const item of detailItems) {
    const li = document.createElement("li");
    li.textContent = item;
    list.appendChild(li);
  }
  el("resultsHeading").focus();
}

async function convert() {
  if (!state.template) {
    showResults("Choose a template RPL first (step 1).", []);
    return;
  }
  if (state.inputs.length === 0) {
    showResults("Add at least one preset file or folder (step 2).", []);
    return;
  }
  if (el("sameFolder").checked) {
    const problem = computeOutput();
    if (problem) {
      showResults(`Step 3: ${problem}`, []);
      return;
    }
  }
  if (!state.output) {
    showResults("Choose where to save the output (step 3).", []);
    return;
  }
  el("convert").disabled = true;
  el("status").textContent = "Converting…";
  try {
    const folderNaming = el("folderMode").hidden
      ? "flat"
      : document.querySelector('input[name="folderMode"]:checked')?.value ?? "flat";
    const summary = await invoke("convert", {
      template: state.template,
      inputs: state.inputs,
      output: state.output,
      folderNaming,
    });
    const details = summary.converted.map((p) => `${p.name}: ${p.parameters} parameters`);
    for (const s of summary.skipped) details.push(`Skipped ${s}`);
    const skippedNote = summary.skipped.length > 0 ? `, ${summary.skipped.length} skipped` : "";
    showResults(
      `Done. Wrote ${summary.converted.length} preset(s)${skippedNote} to ${summary.output}. ` +
        `In REAPER, open the plugin's FX preset menu and choose "Import preset library" to load it.`,
      details
    );
    // Offer to jump straight into the editor on the file just written, so
    // tidying names or order needs no second Browse step.
    state.output = summary.output;
    el("editResult").hidden = false;
  } catch (e) {
    showResults(`Conversion failed: ${e}`, []);
  } finally {
    el("convert").disabled = false;
  }
}

// Land screen reader focus on the app heading once the page is up. The
// window-level focus call in the Rust shell gets keyboard focus into the
// webview; this places the reading cursor at the top of the document. The
// retry covers WebView2 sometimes ignoring focus set in the first frame.
function focusHeading() {
  const heading = el("appTitle");
  heading.focus();
  if (document.activeElement !== heading) {
    setTimeout(() => heading.focus(), 250);
  }
}
window.addEventListener("DOMContentLoaded", focusHeading);
if (document.readyState !== "loading") focusHeading();

// If the window was activated after loading in the background, the load-time
// focus went nowhere. Whenever the webview gains focus with nothing focused
// in the document, land on the heading; a real position is left alone.
window.addEventListener("focus", () => {
  const active = document.activeElement;
  if (!active || active === document.body) focusHeading();
});

// ----- Library editor -----
// Each item points back at the preset's index in the file on disk, so the
// backend can copy blobs by reference and only rebuild renamed ones.
const editor = { path: null, items: [], dirty: false, cut: null };

function editorStatus(text) {
  el("editStatus").textContent = text + (editor.dirty ? " Unsaved changes." : "");
}

function renderPresetList(selection) {
  const list = el("presetList");
  list.textContent = "";
  editor.items.forEach((item, index) => {
    const option = document.createElement("option");
    option.textContent = item.name;
    option.selected = selection.includes(index);
    list.appendChild(option);
  });
}

function selectedIndices() {
  return Array.from(el("presetList").selectedOptions, (o) => o.index);
}

// Load one RPL into the editor. Shared by the Browse button and the
// "open the file I just converted" button in the results area.
async function loadLibraryInto(path) {
  const info = await invoke("load_library", { path });
  editor.path = path;
  editor.items = info.names.map((name, index) => ({ orig: index, name }));
  editor.dirty = false;
  editor.cut = null;
  el("editPath").value = path;
  el("editor").hidden = false;
  renderPresetList([]);
  editorStatus(`Loaded ${info.names.length} presets from ${info.header}.`);
}

async function openLibrary() {
  const chosen = await dialog.open({ title: "Choose RPL to edit", filters: [rplFilter] });
  if (!chosen) return;
  try {
    await loadLibraryInto(chosen);
  } catch (e) {
    editorStatus(`Could not open that library: ${e}`);
  }
}

// Open the freshly converted library without a Browse step, then drop focus
// straight onto the preset list so editing can begin immediately.
async function editConvertedResult() {
  if (!state.output) return;
  try {
    await loadLibraryInto(state.output);
    el("presetList").focus();
  } catch (e) {
    editorStatus(`Could not open the converted library: ${e}`);
  }
}

function moveSelection(direction) {
  const selection = selectedIndices();
  if (selection.length === 0) {
    editorStatus("Select at least one preset to move.");
    return;
  }
  const moved = [];
  let changed = true;
  if (direction === "up" || direction === "down") {
    const step = direction === "up" ? -1 : 1;
    const ordered = direction === "up" ? selection : [...selection].reverse();
    // A selected block resting against the edge stays put; boundary tracks
    // how far the block extends so items don't leapfrog each other.
    let boundary = direction === "up" ? 0 : editor.items.length - 1;
    changed = false;
    for (const index of ordered) {
      if (index === boundary) {
        boundary += direction === "up" ? 1 : -1;
        moved.push(index);
      } else {
        const target = index + step;
        [editor.items[index], editor.items[target]] = [editor.items[target], editor.items[index]];
        moved.push(target);
        changed = true;
      }
    }
  } else {
    const picked = selection.map((i) => editor.items[i]);
    const rest = editor.items.filter((_, i) => !selection.includes(i));
    editor.items = direction === "top" ? [...picked, ...rest] : [...rest, ...picked];
    const base = direction === "top" ? 0 : editor.items.length - picked.length;
    picked.forEach((_, i) => moved.push(base + i));
  }
  if (!changed) {
    editorStatus(direction === "up" ? "Already at the top." : "Already at the bottom.");
    return;
  }
  editor.dirty = true;
  renderPresetList(moved);
  editorStatus(moveAnnouncement(direction, selection.length, moved));
}

// Describe a move with the neighbour it landed against, so arrowing a preset
// through the list gives a sense of place: moving up, name the preset now
// directly below the block; moving down, the one now directly above.
function moveAnnouncement(direction, count, moved) {
  if (direction === "up") {
    const below = Math.max(...moved) + 1;
    if (below < editor.items.length) return `Moved above ${editor.items[below].name}.`;
  } else if (direction === "down") {
    const above = Math.min(...moved) - 1;
    if (above >= 0) return `Moved below ${editor.items[above].name}.`;
  }
  const label = { up: "up", down: "down", top: "to the top", bottom: "to the bottom" }[direction];
  return `Moved ${count} preset(s) ${label}.`;
}

function removeSelected() {
  const selection = selectedIndices();
  if (selection.length === 0) {
    editorStatus("Select at least one preset to remove.");
    return;
  }
  if (selection.length === editor.items.length) {
    editorStatus("Cannot remove every preset; a library needs at least one.");
    return;
  }
  editor.items = editor.items.filter((_, i) => !selection.includes(i));
  editor.dirty = true;
  editor.cut = null; // a pending cut may have just been deleted
  const focusIndex = Math.min(selection[0], editor.items.length - 1);
  renderPresetList([focusIndex]);
  editorStatus(`Removed ${selection.length} preset(s).`);
}

// Pseudo cut-and-paste for reordering. Control+X marks the selected presets
// (they stay in place, like a file manager's cut); Control+V removes them
// from where they are and drops them just below the focused preset, so the
// presets below cascade down to make room.
function cutSelection() {
  const selection = selectedIndices();
  if (selection.length === 0) {
    editorStatus("Select at least one preset to cut.");
    return;
  }
  editor.cut = selection.map((i) => editor.items[i]);
  editorStatus(`Cut ${editor.cut.length} preset(s); press Control+V at the destination.`);
}

function pasteCut() {
  if (!editor.cut || editor.cut.length === 0) {
    editorStatus("Nothing to move. Cut presets first with Control+X.");
    return;
  }
  const cutSet = new Set(editor.cut);
  // Drop point: just below the focused preset. Count the non-cut presets up
  // to and including it, so the target index is right once the cut ones are
  // pulled out. With no selection, drop at the bottom.
  const anchorPos = selectedIndices()[0] ?? editor.items.length - 1;
  let target = 0;
  for (let i = 0; i <= anchorPos; i++) {
    if (!cutSet.has(editor.items[i])) target += 1;
  }
  const remaining = editor.items.filter((item) => !cutSet.has(item));
  const count = editor.cut.length;
  remaining.splice(target, 0, ...editor.cut);
  editor.items = remaining;
  editor.dirty = true;
  editor.cut = null;
  const moved = Array.from({ length: count }, (_, i) => target + i);
  renderPresetList(moved);
  editorStatus(
    target > 0
      ? `Moved ${count} preset(s) below ${editor.items[target - 1].name}.`
      : `Moved ${count} preset(s) to the top.`
  );
}

function duplicateSelected() {
  const selection = selectedIndices();
  if (selection.length === 0) {
    editorStatus("Select at least one preset to duplicate.");
    return;
  }
  const copies = selection.map((i) => ({ ...editor.items[i] }));
  const insertAt = selection[selection.length - 1] + 1;
  editor.items.splice(insertAt, 0, ...copies);
  editor.dirty = true;
  renderPresetList(copies.map((_, i) => insertAt + i));
  editorStatus(`Duplicated ${selection.length} preset(s). Rename the copies to tell them apart.`);
}

function renameSelected(refocusList) {
  const selection = selectedIndices();
  if (selection.length !== 1) {
    editorStatus("Select exactly one preset to rename.");
    return;
  }
  const name = el("renameInput").value.trim();
  if (!name) {
    editorStatus("Type the new name first.");
    return;
  }
  const old = editor.items[selection[0]].name;
  editor.items[selection[0]].name = name;
  editor.dirty = true;
  renderPresetList(selection);
  editorStatus(`Renamed "${old}" to "${name}".`);
  if (refocusList) el("presetList").focus();
}

// F2 flow: jump into the rename field with the current name selected;
// Enter applies and returns to the list, Escape cancels back to the list.
function startRename() {
  const selection = selectedIndices();
  if (selection.length !== 1) {
    editorStatus("Select exactly one preset to rename.");
    return;
  }
  const input = el("renameInput");
  input.value = editor.items[selection[0]].name;
  input.focus();
  input.select();
}

async function saveLibrary() {
  if (!editor.path) return;
  el("saveLibrary").disabled = true;
  try {
    const edits = editor.items.map((item) => ({ originalIndex: item.orig, name: item.name }));
    const count = await invoke("save_library", { path: editor.path, edits });
    // The file now matches the working list, so positions become the new
    // original indices.
    editor.items.forEach((item, index) => (item.orig = index));
    editor.dirty = false;
    editorStatus(`Saved ${count} presets to ${editor.path}.`);
  } catch (e) {
    editorStatus(`Saving failed: ${e}`);
  } finally {
    el("saveLibrary").disabled = false;
  }
}

el("openLibrary").addEventListener("click", openLibrary);
el("editResult").addEventListener("click", editConvertedResult);
el("moveTop").addEventListener("click", () => moveSelection("top"));
el("moveUp").addEventListener("click", () => moveSelection("up"));
el("moveDown").addEventListener("click", () => moveSelection("down"));
el("moveBottom").addEventListener("click", () => moveSelection("bottom"));
el("removeSelected").addEventListener("click", removeSelected);
el("duplicateSelected").addEventListener("click", duplicateSelected);
el("renameButton").addEventListener("click", () => renameSelected(false));
el("saveLibrary").addEventListener("click", saveLibrary);
// Announce multi-selection size as it grows or shrinks; a single selection
// stays quiet since NVDA already reads the focused option itself. The delay
// lets the screen reader finish reading the option that was just reached
// before the count is spoken, and collapses rapid arrowing into one
// announcement of the final count.
let selectionAnnounceTimer;
function announceSelection() {
  clearTimeout(selectionAnnounceTimer);
  selectionAnnounceTimer = setTimeout(() => {
    const count = selectedIndices().length;
    const text = count > 1 ? `${count} presets selected` : "";
    if (el("selectionCount").textContent !== text) {
      el("selectionCount").textContent = text;
    }
  }, 500);
}

// A "fake folder" spans from a "<Name> folder:" marker preset down to just
// before the next marker. The region above the first marker counts too, so
// Control+G is useful even in unmarked areas of the list.
function isFolderMarker(name) {
  return name.includes(" folder: ");
}

function selectCurrentFolder() {
  if (editor.items.length === 0) return;
  const reference = selectedIndices()[0] ?? 0;
  let start = reference;
  while (start > 0 && !isFolderMarker(editor.items[start].name)) {
    start -= 1;
  }
  let end = reference + 1;
  while (end < editor.items.length && !isFolderMarker(editor.items[end].name)) {
    end += 1;
  }
  const options = el("presetList").options;
  for (let i = 0; i < options.length; i++) {
    options[i].selected = i >= start && i < end;
  }
  const marker = editor.items[start].name;
  const folder = isFolderMarker(marker)
    ? `the ${marker.slice(0, marker.indexOf(" folder: "))} folder`
    : "the presets before the first folder";
  editorStatus(`Selected ${end - start} preset(s) in ${folder}.`);
}

// Jump the list cursor to the start of the previous or next fake folder.
// Programmatic selection doesn't speak on its own, so the landing spot is
// announced through the dedicated live region.
function seekFolder(direction) {
  if (editor.items.length === 0) return;
  const current = selectedIndices()[0] ?? 0;
  let target = -1;
  if (direction === "next") {
    for (let i = current + 1; i < editor.items.length; i++) {
      if (isFolderMarker(editor.items[i].name)) {
        target = i;
        break;
      }
    }
  } else {
    for (let i = current - 1; i >= 0; i--) {
      if (isFolderMarker(editor.items[i].name)) {
        target = i;
        break;
      }
    }
  }
  if (target === -1) {
    el("selectionCount").textContent =
      direction === "next" ? "No next folder." : "No previous folder.";
    return;
  }
  landOn(target);
}

// Move the list cursor to one item and announce it; shared by folder
// seeking and find.
function landOn(index, note) {
  const name = editor.items[index].name;
  el("presetList").selectedIndex = index; // also moves the selection anchor
  el("renameInput").value = name;
  el("selectionCount").textContent =
    `${name}, ${index + 1} of ${editor.items.length}${note ? `, ${note}` : ""}`;
}

function findMatch(direction) {
  if (editor.items.length === 0) return;
  const query = el("findInput").value.trim().toLowerCase();
  if (!query) {
    el("selectionCount").textContent = "Type something in the find box first (Control+F).";
    return;
  }
  const total = editor.items.length;
  const current = selectedIndices()[0] ?? (direction === "next" ? -1 : 0);
  const step = direction === "next" ? 1 : -1;
  for (let offset = 1; offset <= total; offset++) {
    const index = (current + step * offset + total * offset) % total;
    if (editor.items[index].name.toLowerCase().includes(query)) {
      const wrapped =
        direction === "next" ? index <= current : index >= current;
      landOn(index, wrapped ? "wrapped" : undefined);
      return;
    }
  }
  el("selectionCount").textContent = `No presets match "${el("findInput").value.trim()}".`;
}

el("presetList").addEventListener("change", () => {
  const selection = selectedIndices();
  if (selection.length === 1) el("renameInput").value = editor.items[selection[0]].name;
  announceSelection();
});
el("presetList").addEventListener("keyup", (e) => {
  if (e.shiftKey && (e.key === "ArrowUp" || e.key === "ArrowDown" || e.key === "Home" || e.key === "End")) {
    announceSelection();
  }
});
el("presetList").addEventListener("keydown", (e) => {
  if (e.key === "Delete") {
    e.preventDefault();
    removeSelected();
  } else if (e.key === "F2") {
    e.preventDefault();
    startRename();
  } else if (e.ctrlKey && e.key === "ArrowUp") {
    e.preventDefault();
    moveSelection("up");
  } else if (e.ctrlKey && e.key === "ArrowDown") {
    e.preventDefault();
    moveSelection("down");
  } else if (e.ctrlKey && e.key === "Home") {
    e.preventDefault();
    moveSelection("top");
  } else if (e.ctrlKey && e.key === "End") {
    e.preventDefault();
    moveSelection("bottom");
  } else if (e.ctrlKey && (e.key === "s" || e.key === "S")) {
    e.preventDefault();
    saveLibrary();
  } else if (e.ctrlKey && (e.key === "x" || e.key === "X")) {
    e.preventDefault();
    cutSelection();
  } else if (e.ctrlKey && (e.key === "v" || e.key === "V")) {
    e.preventDefault();
    pasteCut();
  } else if (e.shiftKey && (e.key === "F" || e.key === "f")) {
    e.preventDefault();
    selectCurrentFolder();
  } else if (e.altKey && e.key === "ArrowUp") {
    e.preventDefault();
    seekFolder("previous");
  } else if (e.altKey && e.key === "ArrowDown") {
    e.preventDefault();
    seekFolder("next");
  } else if (e.ctrlKey && (e.key === "f" || e.key === "F")) {
    e.preventDefault();
    el("findInput").focus();
    el("findInput").select();
  } else if (e.key === "F3") {
    e.preventDefault();
    findMatch(e.shiftKey ? "previous" : "next");
  }
});
el("findInput").addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    findMatch(e.shiftKey ? "previous" : "next");
    el("presetList").focus();
  } else if (e.key === "F3") {
    e.preventDefault();
    findMatch(e.shiftKey ? "previous" : "next");
    el("presetList").focus();
  } else if (e.key === "Escape") {
    e.preventDefault();
    el("presetList").focus();
  }
});
el("findNext").addEventListener("click", () => findMatch("next"));
el("findPrevious").addEventListener("click", () => findMatch("previous"));
// F1 anywhere in the editor opens keyboard help; key events from the list
// and text fields bubble up to this container.
el("editor").addEventListener("keydown", (e) => {
  if (e.key === "F1") {
    e.preventDefault();
    el("keyboardHelp").open = true;
    el("helpSummary").focus();
  }
});
el("keyboardHelp").addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    e.preventDefault();
    el("keyboardHelp").open = false;
    el("presetList").focus();
  }
});
el("renameInput").addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    renameSelected(true);
  } else if (e.key === "Escape") {
    e.preventDefault();
    el("presetList").focus();
  }
});

el("pickTemplate").addEventListener("click", pickTemplate);
el("addFiles").addEventListener("click", addFiles);
el("addFolder").addEventListener("click", addFolder);
el("clearInputs").addEventListener("click", clearInputs);
el("scanPlugin").addEventListener("click", scanPlugin);
el("pickOutput").addEventListener("click", pickOutput);
el("sameFolder").addEventListener("change", onSameFolderToggle);
el("outputName").addEventListener("input", onOutputNameInput);
el("convert").addEventListener("click", convert);
