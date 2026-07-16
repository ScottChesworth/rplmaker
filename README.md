# rplMaker

Converts plugin preset files into REAPER preset libraries (RPL), so presets
buried behind inaccessible plugin GUIs can be browsed from REAPER's
screen-reader-friendly native preset combobox instead.

New here? The end-user walkthrough of the desktop app, including the preset
editor and its full keyboard reference, is in [GUIDE.md](GUIDE.md). The rest
of this file is the technical and build reference.

## How it works

Many plugins (Neural DSP among them) store presets as serialized JUCE data,
and their VST3 state is XML wrapped in a small binary envelope. rplMaker is
template-based:

1. In REAPER, save any one preset for the plugin through the FX preset
   combobox, then choose "Export preset library" from the preset menu.
2. Feed that exported RPL to rplMaker as the template. It teaches the
   converter the plugin-specific wrapper bytes that don't exist in the
   vendor's preset files.
3. Point rplMaker at the vendor's preset files (or a whole folder). It swaps
   each preset's parameters into the template and writes one RPL containing
   them all.
4. Back in REAPER, use "Import preset library" from the FX preset menu.

Converting a preset that REAPER itself captured reproduces REAPER's own RPL
byte-for-byte; that round trip is enforced by the test suite using the files
in `Examples/`.

## Layout

- `crates/rplmaker-core` — the converter library: JUCE ValueTree and XML
  readers/writers, RPL reader/writer, VST3 blob templating and wrapper
  peeling, XML and tree parameter merges, library editing, and
  embedded-preset extraction.
- `crates/rplmaker-cli` — command-line front end (`rplmaker`).
- `src-tauri` plus `ui/` — the accessible Tauri GUI.
- `Scripts/` — the ReaScript for capturing presets that only REAPER can
  reach.
- `Examples/` — real preset files and hand-saved RPLs used as test ground
  truth.
- `GUIDE.md` — the end-user walkthrough.

## Getting the app

Portable builds for Windows and macOS are attached to every
[release](../../releases), and to every CI run under "Artifacts". Nothing
installs: unzip and run.

- Windows (`rplMaker-windows-x64-portable.zip`) — run `rplMaker.exe`. It
  needs the Microsoft WebView2 runtime, which ships with Windows 10 and 11
  already; on an older machine, install the Evergreen runtime from Microsoft.
- macOS (`rplMaker-macos-universal-portable.zip`) — universal, so it runs on
  both Apple Silicon and Intel. The build is unsigned, so the first launch
  needs a right-click (or Control-click) on `rplMaker.app` and then "Open",
  which offers an "Open" button that a normal double-click does not. Double
  -clicking first will just say the app is damaged or from an unidentified
  developer. Alternatively run `xattr -cr /path/to/rplMaker.app` once.

Each zip also contains the command-line converter alongside the app:
`rplmaker-cli.exe` on Windows, `rplmaker` on macOS.

## Building

- Tests: `cargo test`. The integration tests read the real preset files in
  `Examples/`, which are vendor factory presets and so are not committed; a
  fresh clone has no copy, and those tests skip themselves (including in CI).
  Supply your own templates and preset files there to run them for real.
- CLI: `cargo build --release -p rplmaker-cli`, binary at
  `target/release/rplmaker.exe`
- GUI: `cargo build --release -p rplmaker-gui`, or `cargo tauri build` for
  installers (requires `cargo install tauri-cli`)

## CLI usage

```
rplmaker --template "My Plugin.RPL" --output "Converted.RPL" path\to\presets
```

Inputs may be individual preset files, folders (searched recursively), or a
mix. Presets that fail to convert are reported and skipped. Files are
ordered case-insensitively by path, so presets stay grouped by subfolder.

To pull presets straight out of a plugin binary that ships no preset files,
add `--scan-plugin` (with or without other inputs):

```
rplmaker --template "My Plugin.RPL" --output "Factory.RPL" \
  --scan-plugin "C:\Program Files\Common Files\VST3\My Plugin.vst3"
```

Add `--folder-markers` to prepend a folder marker to the first preset of
each subfolder ("Ambient folder: Dark Cave"); arrowing through REAPER's
flat preset list then announces where each folder starts. It takes an
optional style: `deepest` (the default, innermost folder name only) or
`full` (the whole relative path, "Artists, Adam Christianson folder:
Dreamolo"). The GUI offers the same choices automatically whenever the
chosen inputs contain subfolders.

In the GUI, step 3 (Output) can either save beside the template — tick "Save
in the same folder as the template" and type a file name — or open a normal
save dialog.

## Editing libraries

The GUI can also open a finished RPL and rename, reorder, remove, or
duplicate its presets, then save back to the same file. Renames are applied
inside each preset's binary blob as well as on the library entry, so edited
files keep working as conversion templates. Saves go through a temp file
and rename, so a failed write can't corrupt the library.

The editor is keyboard-driven — move, multi-select, folder-jump, find,
rename, remove, and save all have shortcuts. The full list is in
[GUIDE.md](GUIDE.md#editor-keyboard-reference) and in the app's own F1
keyboard help.

## Scanning a plug-in for embedded presets

Many JUCE plugins compile their factory presets into the plugin binary, so
no preset files exist anywhere on disk. Once a template is loaded, rplMaker
knows the plugin's state root element and can dig those presets out of the
binary directly: use "Scan a plug-in for embedded presets" in the GUI's
step 2, or `--scan-plugin "C:\...\Plugin.vst3"` on the CLI. Found presets
get positional names ("Plugin 001" and so on) because vendors store the
real names in undocumented metadata; the library editor's rename tools
cover the favorites. Truncated documents are skipped and duplicates
removed. Plug-ins that compress their embedded resources will yield
nothing — the scan says so rather than guessing.

## Capturing factory presets without preset files

Some plugins embed their factory presets in the plugin binary or a private
database, so there are no files to convert. For those, run
`Scripts/rplMaker capture factory presets.lua` as a ReaScript inside
REAPER: insert the plugin as the first FX on a selected track and run the
script. It steps through every preset the plugin exposes to REAPER,
captures the state of each, and writes a ready-to-import RPL into the
REAPER resource path (the exact location is announced when it finishes).
The result can be opened in rplMaker's editor for renaming and reordering
like any other library. This route works for any VST2 or VST3 plugin that
publishes a preset list, whether or not its presets exist as files.

## Supported plugins

VST3 (and VST2-state) plugins built on JUCE. The blob layer peels the
wrappers REAPER and JUCE put around plugin state and reaches the state in any
of these forms:

- XML text behind a "VC2!" marker (e.g. Archetype John Mayer X)
- raw binary ValueTree (e.g. Archetype Gojira)
- XML wrapped in a Steinberg `VstW` header plus a VST2 `fxBank`/`fxProgram`
  chunk (e.g. Mercuriall Ampbox)

The XML merge matches repeated elements (JUCE's `PARAM` nodes) by `id` and
then by position, so APVTS-style states convert correctly. Vendor preset
files are accepted as JUCE binary ValueTrees or as XML text.

Universal Audio's UADx plugins are also supported, though they are not JUCE.
Their component state is a UBJSON object whose `plugin_state_payload` member
holds the real parameters as a JSON text string; the converter parses and
rebuilds that UBJSON (`ubjson.rs`) losslessly. Their preset files are JSON
(`uapreset.rs`), one per preset, with the parameters under a `chunk` object
that maps straight onto the payload. UADx factory presets ship as such files
inside the plugin bundle, e.g. `...\uaudio_verve_essentials.lunacomponent\
algo.bundle\Contents\Resources\presets\*.json`; point the converter at that
folder. Unlike JUCE plugins, nothing is embedded in the binary to scan, so
the "scan a plug-in" route does not apply to UADx.

Not supported: plugins built on other engines that store presets in a
locked or proprietary format. GGD's Modern & Massive 2 (Cradle engine) is a
known example — its state is readable but its factory presets are encrypted
on disk, so there is no preset source to convert. New JUCE state shapes can
be added in `rplmaker-core` (`blob.rs` for wrappers, `xmlmerge`/`treemerge`
for state).
