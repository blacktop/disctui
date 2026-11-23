-- Rex actions for disctui.
--
-- Install:
--   disctui rex > ~/.config/rex/disctui.lua
-- then add this line to ~/.config/rex/init.lua and run `rex config reload`:
--   dofile(os.getenv("HOME") .. "/.config/rex/disctui.lua")
--
-- Actions (command palette, `rex do NAME`, or a key):
--   disctui_jump    search every channel and DM, open the pick in disctui
--   disctui_inbox   list what needs you, open the pick in disctui
--   disctui_digest  tick servers, then summarize them in a split
--
-- Example bindings:
--   rex.bind("alt+cmd+j", "disctui_jump")
--   rex.bind("alt+cmd+i", "disctui_inbox")
--   rex.bind("alt+cmd+d", "disctui_digest")
--
-- Each action takes its answer as an argument too, so scripts can skip the
-- prompt: `rex do disctui_jump channel=123456789`.
--
-- Set DISCTUI_BIN in the Rex server's environment when `disctui` is not on
-- its PATH.

local TERMINAL = "com.superlogical.terminal"
local SHELL = "com.superlogical.terminal.shell"
local APP = "disctui"
local BIN = os.getenv("DISCTUI_BIN") or "disctui"
-- rex.ui.pick shows at most 2,000 items.
local MAX_ITEMS = 2000

local function fail(message)
  error(APP .. ": " .. message, 0)
end

-- Rex actions have 30 seconds. Run the CLI in a transient terminal and
-- invoke the picker in the original context after its JSON is available.
-- JSON travels over stdin. No channel names or messages become shell code.
local LOADER = [=[
set -euo pipefail
prefix=$1; action=$2; session=$3; block=$4; client=$5
shift 5
target=(do "$action" -s "$session" --args -)
if [[ -n "$block" ]]; then target+=(-b "$block"); fi
if [[ -n "$client" ]]; then target=(-C "$client" "${target[@]}"); fi
printf 'Loading Discord data… (Ctrl-C to cancel)\n'
# Read at most 8 MiB plus one byte, including for a noisy command.
if data=$("$@" | head -c 8388609) && (LC_ALL=C; (( ${#data} < 8388609 ))); then
    if printf '%s%s}' "$prefix" "$data" | rex "${target[@]}"; then exit 0; fi
fi
read -r -p 'Could not load the picker. Press Return to close.' || true
exit 1
]=]

local function load_in_split(ctx, name, args, argv)
  local prefix = "{"
  if next(args) ~= nil then
    prefix = rex.json.encode(args):sub(1, -2) .. ","
  end
  local command = { "/bin/bash", "-c", LOADER, "disctui-picker", prefix .. '"prepared":',
                    name, ctx.session_id, ctx.block_id or "", ctx.client_id or "" }
  for _, arg in ipairs(argv) do command[#command + 1] = arg end
  local split, err = rex.session.new_split({
    anchor_id = ctx.block_id, direction = "horizontal", side = "after",
    layout = rex.layout.block({ flavor = SHELL, options = {
      command = command, exit = { on_completion = true },
    } }),
  })
  if err ~= nil then fail(err) end
  return { block_id = split.block_ids[1] }
end

local function disctui_argv(mock, ...)
  local argv = { BIN, ... }
  if mock then
    argv[#argv + 1] = "--mock"
  end
  return argv
end

-- The terminal block whose program reported itself as disctui (OSC 7501).
local function find_disctui_block()
  local listing = rex.session.list_blocks({})
  if listing == nil then
    return nil
  end
  for _, block in ipairs(listing.blocks) do
    if block.creator_name == TERMINAL and block.block_id ~= nil then
      local status = rex.block.call(TERMINAL, "program_status", { block_id = block.block_id })
      for _, record in ipairs(status and status.records or {}) do
        if record.effective_app == APP or record.app == APP then
          return block.block_id
        end
      end
    end
  end
  return nil
end

-- Open a channel in the running disctui, or start disctui on that channel.
local function open_channel(channel_id)
  local sent = rex.exec({ BIN, "open", channel_id }, { timeout = 10 })
  if sent ~= nil and sent.ok then
    local block_id = find_disctui_block()
    if block_id ~= nil then
      rex.session.focus_block({ block_id = block_id })
    end
    return { channel_id = channel_id, started = false, block_id = block_id }
  end

  local window, err = rex.session.new_window({
    window_label = APP,
    layout = rex.layout.block({
      flavor = SHELL,
      options = { command = { BIN, "--open", channel_id } },
    }),
  })
  if err ~= nil then
    fail(err)
  end
  return { channel_id = channel_id, started = true, block_id = window.block_ids[1] }
end

-- nil means the person closed the prompt; anything else that went wrong raises.
local function answer_or_fail(answer, why, detail)
  if answer ~= nil or why == "cancelled" then
    return answer
  end
  fail((why or "prompt failed") .. (detail and (": " .. detail) or ""))
end

local function channel_title(channel)
  return (channel.kind == "dm" and "@" or "#") .. channel.name
end

local function channel_item(channel)
  local subtitle = channel.guild
  if channel.category ~= nil then
    subtitle = subtitle .. " · " .. channel.category
  end
  local detail = nil
  if channel.mentions > 0 then
    detail = channel.mentions .. (channel.mentions == 1 and " mention" or " mentions")
  elseif channel.unread then
    detail = "unread"
  end
  return {
    id = channel.id,
    title = channel_title(channel),
    subtitle = subtitle,
    icon = channel.kind == "dm" and "person" or "tag",
    detail = detail,
  }
end

-- Unread channels first, each group in sidebar order.
local function channel_items(channels)
  local unread, read = {}, {}
  for _, channel in ipairs(channels) do
    local bucket = (channel.unread or channel.mentions > 0) and unread or read
    bucket[#bucket + 1] = channel_item(channel)
  end
  local items = unread
  for _, item in ipairs(read) do
    items[#items + 1] = item
  end
  if #items > MAX_ITEMS then
    rex.log("warn", APP .. ": showing the first " .. MAX_ITEMS .. " of " .. #items .. " channels")
    for index = #items, MAX_ITEMS + 1, -1 do
      items[index] = nil
    end
  end
  return items
end

rex.action({
  name = "disctui_jump",
  title = "disctui: Jump to Channel…",
  description = "Search every Discord channel and DM, then open the pick in disctui.",
  category = "disctui",
  keywords = "discord channel dm search",
  args = { prepared = "array?", channel = "string?", query = "string?", mock = "boolean?" },
  run = function(ctx, args)
    local channel_id = args.channel
    if channel_id == nil then
      local channels = args.prepared
      if channels == nil then
        return load_in_split(ctx, "disctui_jump", args, disctui_argv(args.mock, "channels", "--format", "json"))
      end
      channel_id = answer_or_fail(rex.ui.pick({
        title = "Jump to channel",
        placeholder = "Filter channels and DMs",
        value = args.query,
        confirm = "Open in disctui",
        items = channel_items(channels),
      }))
      if channel_id == nil then
        return
      end
    end
    return open_channel(channel_id)
  end,
})

rex.action({
  name = "disctui_inbox",
  title = "disctui: Needs You…",
  description = "List mentions and unanswered DMs from the last day, then open the pick in disctui.",
  category = "disctui",
  keywords = "discord mentions inbox unread",
  args = { prepared = "object?", since = "string?", mock = "boolean?" },
  run = function(ctx, args)
    local report = args.prepared
    if report == nil then
      return load_in_split(ctx, "disctui_inbox", args, disctui_argv(
        args.mock, "digest", "--no-ai", "--format", "json", "--since", args.since or "24h", "--limit", "10"
      ))
    end

    -- Several items can point at one channel, so pick ids are list positions.
    local items = {}
    for index, item in ipairs(report.needs_you) do
      items[#items + 1] = {
        id = tostring(index),
        title = item.snippet,
        subtitle = item.guild .. " " .. item.channel .. " · " .. item.author .. " " .. item.time,
        icon = string.sub(item.channel, 1, 1) == "@" and "person" or "tag",
        detail = item.reason,
      }
    end
    if #items == 0 then
      rex.log("info", APP .. ": nothing needs you in the " .. report.window)
      return { needs_you = 0 }
    end

    local picked = answer_or_fail(rex.ui.pick({
      title = "Needs you (" .. report.window .. ")",
      placeholder = "Filter",
      confirm = "Open in disctui",
      items = items,
    }))
    if picked == nil then
      return
    end
    return open_channel(report.needs_you[tonumber(picked)].channel_id)
  end,
})

-- One entry per server, in sidebar order, with how many of its channels are unread.
local function server_items(channels)
  local by_id, items = {}, {}
  for _, channel in ipairs(channels) do
    local server = by_id[channel.guild_id]
    if server == nil then
      server = { id = channel.guild_id, name = channel.guild, unread = 0 }
      by_id[channel.guild_id] = server
      items[#items + 1] = server
    end
    if channel.unread or channel.mentions > 0 then
      server.unread = server.unread + 1
    end
  end
  for index, server in ipairs(items) do
    items[index] = {
      id = server.id,
      title = server.name,
      icon = server.id == "@me" and "person" or "server",
      detail = server.unread > 0 and (server.unread .. " unread") or "nothing unread",
      selected = server.unread > 0,
    }
  end
  return items
end

rex.action({
  name = "disctui_digest",
  title = "disctui: Digest Servers…",
  description = "Tick the servers to summarize, then read the digest in a split.",
  category = "disctui",
  keywords = "discord summary digest ai",
  args = { prepared = "array?", guilds = "array?", since = "string?", mock = "boolean?" },
  run = function(ctx, args)
    local guilds = args.guilds
    if guilds == nil then
      local channels = args.prepared
      if channels == nil then
        return load_in_split(ctx, "disctui_digest", args, disctui_argv(args.mock, "channels", "--format", "json"))
      end
      guilds = answer_or_fail(rex.ui.pick({
        title = "Digest servers",
        placeholder = "Filter servers",
        multiple = true,
        confirm = "Summarize",
        items = server_items(channels),
      }))
      if guilds == nil or #guilds == 0 then
        return
      end
    end

    local command = disctui_argv(args.mock, "digest", "--since", args.since or "24h")
    for _, guild in ipairs(guilds) do
      command[#command + 1] = "--guild"
      command[#command + 1] = guild
    end

    local split, err = rex.session.new_split({
      direction = "horizontal",
      side = "after",
      anchor_id = ctx.block_id,
      layout = rex.layout.block({
        flavor = SHELL,
        -- Keep the digest on screen after disctui exits.
        options = { command = command, exit = { on_completion = false } },
      }),
    })
    if err ~= nil then
      fail(err)
    end
    return { block_id = split.block_ids[1], guilds = guilds }
  end,
})
