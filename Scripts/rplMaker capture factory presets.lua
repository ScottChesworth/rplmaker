--[[
rplMaker: capture a plugin's factory presets into an RPL preset library.

For plugins whose presets are embedded in the binary or otherwise not
stored as convertible files. Walks every preset the plugin exposes to
REAPER, captures the FX state for each, and writes a standard RPL that
REAPER can import ("Import preset library" in the FX preset menu) and that
rplMaker's editor can rename and reorder.

Setup: insert the plugin as the FIRST effect on a track, select that
track, then run this script. The RPL is written into the REAPER resource
path and the exact location is announced when done.

Note: the preset list REAPER exposes includes factory presets plus any
user presets already saved for the plugin; run this on a freshly
installed plugin for a clean factory set.
]]

local function message(text)
  reaper.ShowMessageBox(text, "rplMaker capture", 0)
end

-- Match REAPER's own RPL quoting: backticks unless the name uses them.
local function quote_name(name)
  for _, q in ipairs({ "`", '"', "'" }) do
    if not name:find(q, 1, true) then
      return q .. name .. q
    end
  end
  return "`" .. name:gsub("`", "'") .. "`"
end

-- Pull the first FX's base64 state out of the track chunk. The
-- concatenated base64 lines of a <VST block are byte-identical to what an
-- RPL <PRESET entry stores.
local function capture_fx_base64(track)
  local ok, chunk = reaper.GetTrackStateChunk(track, "", false)
  if not ok then
    return nil
  end
  local collected = {}
  local in_vst = false
  for line in chunk:gmatch("[^\r\n]+") do
    if in_vst then
      local b64 = line:match("^%s*([A-Za-z0-9+/=]+)%s*$")
      if b64 then
        collected[#collected + 1] = b64
      elseif line:match("^%s*>%s*$") then
        break
      end
    elseif line:match("^%s*<VST[%s3]") then
      in_vst = true
    end
  end
  if #collected == 0 then
    return nil
  end
  return table.concat(collected)
end

-- Re-wrap continuous base64 to REAPER's RPL layout: 128 chars per line,
-- four-space indent, CRLF endings.
local function wrapped_base64_lines(b64)
  local lines = {}
  for i = 1, #b64, 128 do
    lines[#lines + 1] = "    " .. b64:sub(i, i + 127)
  end
  return table.concat(lines, "\r\n")
end

local function main()
  local track = reaper.GetSelectedTrack(0, 0)
  if not track then
    return message("Select the track holding the plugin first.")
  end
  if reaper.TrackFX_GetCount(track) == 0 then
    return message("The selected track has no FX. Insert the plugin as the first effect.")
  end
  local fx = 0
  local _, fx_name = reaper.TrackFX_GetFXName(track, fx, "")
  local original_index, preset_count = reaper.TrackFX_GetPresetIndex(track, fx)
  if preset_count == 0 then
    return message(
      fx_name .. " exposes no presets to REAPER.\n\n" ..
      "This plugin keeps its preset list private, so the capture route " ..
      "cannot reach it; its presets can only be saved manually through " ..
      "the plugin's own interface."
    )
  end

  local presets = {}
  local failures = 0
  for i = 0, preset_count - 1 do
    reaper.TrackFX_SetPresetByIndex(track, fx, i)
    local _, preset_name = reaper.TrackFX_GetPreset(track, fx, "")
    if preset_name == "" then
      preset_name = string.format("Preset %d", i + 1)
    end
    local b64 = capture_fx_base64(track)
    if b64 then
      presets[#presets + 1] = { name = preset_name, b64 = b64 }
    else
      failures = failures + 1
    end
  end
  -- Leave the plugin on the preset it started with.
  if original_index >= 0 then
    reaper.TrackFX_SetPresetByIndex(track, fx, original_index)
  end

  if #presets == 0 then
    return message("No presets could be captured; the FX state was not readable from the track chunk.")
  end

  local out = {}
  out[#out + 1] = '<REAPER_PRESET_LIBRARY "' .. fx_name .. '"'
  for _, preset in ipairs(presets) do
    out[#out + 1] = "  <PRESET " .. quote_name(preset.name)
    out[#out + 1] = wrapped_base64_lines(preset.b64)
    out[#out + 1] = "  >"
  end
  out[#out + 1] = ">"
  out[#out + 1] = ""

  local safe_name = fx_name:gsub('[\\/:*?"<>|]', "-")
  local path = reaper.GetResourcePath() .. "/" .. safe_name .. " factory presets.RPL"
  local file, err = io.open(path, "wb")
  if not file then
    return message("Could not write the output file:\n" .. tostring(err))
  end
  file:write(table.concat(out, "\r\n"))
  file:close()

  local summary = string.format("Captured %d preset(s) to:\n%s", #presets, path)
  if failures > 0 then
    summary = summary .. string.format("\n\n%d preset(s) could not be captured.", failures)
  end
  message(summary)
end

main()
