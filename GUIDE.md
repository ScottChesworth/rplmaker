# rplMaker user guide

rplMaker makes a plug-in's presets reachable from REAPER's own preset
combobox, which works with a screen reader, instead of the plug-in's
inaccessible preset browser. You do the work once and share the result, so
no one else has to repeat it.

This guide covers the desktop app. Everything here is keyboard-navigable and
built to be announced clearly by NVDA and VoiceOver; the preset editor in
particular is driven entirely from the keyboard.

## The idea in one paragraph

REAPER can save and recall presets through a plain combobox that a screen
reader reads fine. Most plug-ins also have their own preset browser, and
those are usually unlabelled graphics. rplMaker converts a plug-in's presets
into REAPER's own preset library format (an `.rpl` file). Import that file
once and every preset shows up in the accessible combobox.

## What you need first: a template

rplMaker learns each plug-in from a template — one preset you save through
REAPER itself. This teaches it the hidden, plug-in-specific data that the
preset files alone don't contain. You only make a template once per plug-in,
and it can be shared.

To make one:

1. Add the plug-in to a track in REAPER.
2. In the FX window, open the preset combobox and choose "Save preset as
   default" or save any preset, so at least one preset exists.
3. From the same preset menu, choose "Export preset library" and save the
   `.rpl` somewhere you can find it.

That exported file is your template.

## Converting presets: the three steps

The main window is laid out as three numbered steps, top to bottom.

### Step 1: Template

Activate "Browse for template RPL", pick the template you exported. The app
confirms which plug-in it is for, e.g. "Presets will be created for VST3:
Archetype John Mayer X (Neural DSP)."

### Step 2: Presets to convert

Add the presets you want. Three buttons:

- "Add preset files" — pick individual preset files.
- "Add a folder" — pick a folder; every preset inside it, including
  subfolders, is included.
- "Scan a plug-in for embedded presets" — for plug-ins that ship no preset
  files at all, this digs the factory presets straight out of the plug-in
  binary. See "When there are no preset files" below.

If the presets you added contain subfolders, a set of choices appears for
how folder names should show up in the list — see "Folders" below.

### Step 3: Output

Choose where to save the finished library. Either tick "Save in the same
folder as the template" and type a file name, or activate "Choose where to
save" for a normal save dialog.

Then activate "Convert". The Results area reports how many presets were
converted, how many were skipped and why, and where the file was written. In
REAPER, open the plug-in's FX preset menu and choose "Import preset library"
to load it. The Results area also has an "Open this library in the editor"
button, which loads the file you just wrote straight into the preset editor
below — handy for tidying names or order without browsing for it again.

## Folders

REAPER's preset list is flat — it has no folders. When your presets come
from subfolders, rplMaker can fold the folder name into the preset name so
you can still hear where each group starts while arrowing through the list.
When subfolders are detected, step 2 offers three choices:

- One straight list — every preset keeps its own name.
- Announce folders by name — the first preset of each subfolder gets that
  folder's name in front, like "Ambient folder: Dark Cave".
- Announce folders by full path — the marker spells out nested folders, like
  "Artists, Adam Christianson folder: Dreamolo".

The marker lands only on the first preset of each folder; the rest keep their
plain names, so you hear the folder change without every entry getting
longer.

## Editing a library

Below the converter is "Edit a preset library", for tidying a finished
`.rpl` — renaming, reordering, removing, or duplicating presets. This is how
you turn a raw conversion into something pleasant to navigate: give presets
shorter names, group the ones you actually use at the top, drop duplicates.

Activate "Open RPL to edit" and pick a library. The presets appear in a
multi-select list. Renames are written into the preset data itself, not just
the visible name, so an edited library still works as a template. Saving goes
through a temporary file first, so a failed save can never corrupt the file
you started from.

### Editor keyboard reference

All of these work while the preset list has focus. Press F1 in the app for
the same list at any time.

| Key | Action |
| --- | --- |
| Up / Down arrows | Move through the list |
| Shift or Control with arrows | Select more than one preset |
| Control+Up / Control+Down | Move the selected presets up or down |
| Control+Home / Control+End | Move the selection to the top or bottom |
| Control+X then Control+V | Cut the selected presets, then drop them below the focused preset |
| Alt+Up / Alt+Down | Jump to the start of the previous or next folder |
| Shift+F | Select the whole folder around the current preset |
| Control+F | Jump to the find box (Enter searches, Shift+Enter backwards) |
| F3 / Shift+F3 | Repeat the last search forwards or backwards |
| F2 | Rename the selected preset (Enter applies, Escape cancels) |
| Delete | Remove the selected presets |
| Control+S | Save changes |
| F1 | Open the keyboard help; Escape closes it and returns to the list |

When you extend a selection with Shift and the arrows, the number of selected
presets is announced once you stop moving.

## When there are no preset files

Some plug-ins keep their factory presets inside the plug-in binary rather
than as files on disk. rplMaker has two ways to reach those.

- Scan the plug-in (step 2, "Scan a plug-in for embedded presets"). With a
  template loaded, rplMaker reads the plug-in file directly and pulls out
  every preset it can find. These arrive with numbered names like
  "Plugin 001", because the plug-in doesn't store the real names in a form
  anything can read back; rename the ones you care about in the editor.
- The capture script, for plug-ins that expose their presets to REAPER but
  not as files. See `Scripts/rplMaker capture factory presets.lua` and the
  README for how to run it.

A plug-in that encrypts or compresses its presets won't yield anything to
either method — the scan will tell you so rather than guess.

## Which plug-ins work

rplMaker works with plug-ins built on the JUCE framework, which is most of
them. It reads both of JUCE's ways of storing state (XML and binary), and
copes with the extra wrapping some plug-ins add. Confirmed working so far:
Neural DSP's Archetype line (John Mayer X, Gojira), Mercuriall Ampbox, and
Universal Audio's UADx range.

Universal Audio's UADx plug-ins work too. Their factory presets ship as JSON
files inside the plug-in itself; point step 2 at the plug-in's presets
folder, for example `C:\Program Files\Common Files\Universal Audio\Plug-Ins\
<plugin>.lunacomponent\algo.bundle\Contents\Resources\presets`. Every UADx
plug-in keeps its presets in the same place, so the same steps convert any of
them. Your own saved UADx presets in `Documents\Universal Audio\Presets` work
the same way.

Some plug-ins use other engines that store presets in a locked format — GGD's
Modern & Massive 2, built on the Cradle engine, is one — and those can't be
batch-converted. For them, saving presets one at a time through REAPER
remains the only route, but even then the sharing and editing parts of
rplMaker still help.

If a plug-in you use doesn't work, that's usually a new format worth
investigating rather than a permanent no.
