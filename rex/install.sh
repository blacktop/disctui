#!/usr/bin/env bash
# Install the disctui actions into Rex: write the actions file beside Rex's
# init.lua, load it from init.lua, and reload the running Rex server.
#
# Usage: rex/install.sh
#
# Environment:
#   DISCTUI_BIN      The disctui to register. Default: the first disctui on
#                    PATH that has the `rex` command.
#   REX_CONFIG       Rex's init.lua, when it is not in the default place.
#   XDG_CONFIG_HOME  Rex's configuration root, as Rex itself reads it.
#
# The installed actions call that disctui by its full path, so they work when
# the Rex server's PATH differs from the shell's. Safe to run again, for
# example after upgrading disctui.
set -euo pipefail

fail() {
	printf 'rex/install.sh: %s\n' "$1" >&2
	exit 1
}

has_rex_command() {
	"$1" rex >/dev/null 2>&1
}

absolute() {
	case $1 in
	/*) printf '%s\n' "$1" ;;
	*) printf '%s/%s\n' "$PWD" "$1" ;;
	esac
}

# Print the disctui to register, as an absolute path.
find_disctui() {
	local candidate
	if [[ -n ${DISCTUI_BIN:-} ]]; then
		candidate=$(command -v "$DISCTUI_BIN") ||
			fail "DISCTUI_BIN=$DISCTUI_BIN is not an executable"
		has_rex_command "$candidate" ||
			fail "$candidate has no \`rex\` command; it predates the Rex actions"
		absolute "$candidate"
		return
	fi
	while IFS= read -r candidate; do
		if has_rex_command "$candidate"; then
			absolute "$candidate"
			return
		fi
	done < <(type -aP disctui || true)
	fail "no disctui with the \`rex\` command on PATH; upgrade disctui or set DISCTUI_BIN"
}

# The paths below are written into Lua string literals.
require_lua_quotable() {
	case $1 in
	*\"* | *\\* | *$'\n'*) fail "cannot write this path into Lua: $1" ;;
	esac
}

check_config() {
	local report
	if ! report=$(rex config check "$1" 2>&1); then
		printf '%s\n' "$report" >&2
		fail "Rex rejected the candidate; installed files were not changed"
	fi
}

if [[ $# -gt 0 ]]; then
	sed -n '2,15s/^# \{0,1\}//p' "$0"
	[[ $1 == -h || $1 == --help ]] && exit 0
	exit 2
fi

command -v rex >/dev/null || fail "the rex command is not on PATH"
bin=$(find_disctui)
init=${REX_CONFIG:-${XDG_CONFIG_HOME:-$HOME/.config}/rex/init.lua}
config_dir=$(dirname "$init")
actions=$config_dir/disctui.lua
require_lua_quotable "$bin"
require_lua_quotable "$actions"

# Point the actions at this disctui instead of whatever Rex finds on its PATH.
lua=$("$bin" rex | DISCTUI_PIN=$bin awk '
	/^local BIN = / {
		printf "local BIN = os.getenv(\"DISCTUI_BIN\") or \"%s\"\n", ENVIRON["DISCTUI_PIN"]
		pinned = 1
		next
	}
	{ print }
	END { if (!pinned) exit 1 }
') || fail "the actions printed by $bin have no BIN line to point at it"

mkdir -p "$config_dir"
# Stage the actions on the same filesystem so the final rename is atomic.
[[ ! -d $actions ]] || fail "$actions is a directory"
staged_actions=$(mktemp "$config_dir/.disctui-actions.XXXXXXXX")
candidate=''
preview=''
cleanup() {
	rm -f "$staged_actions"
	if [[ -n $preview ]]; then rm -f "$preview"; fi
	if [[ -n $candidate ]]; then rm -f "$candidate"; fi
}
trap cleanup EXIT
printf '%s\n' "$lua" >"$staged_actions"
check_config "$staged_actions"

loader_pattern='^[[:space:]]*dofile[(].*disctui[.]lua'
if [[ -f $init ]] && grep -Eq "$loader_pattern" "$init"; then
	proposed=$init
else
	# Preserve a dotfiles-managed symlink by updating its target.
	init_target=$init
	if [[ -L $init ]]; then init_target=$(realpath "$init"); fi
	candidate=$(mktemp "$(dirname "$init_target")/.disctui-init.XXXXXXXX")
	if [[ -f $init ]]; then cp -pL "$init" "$candidate"; fi
	case $actions in
	"$HOME"/*) load="dofile(os.getenv(\"HOME\") .. \"${actions#"$HOME"}\")" ;;
	*) load="dofile(\"$actions\")" ;;
	esac
	printf '\n%s\n%s\n' '-- disctui: command palette actions (rex/install.sh).' "$load" >>"$candidate"
	proposed=$candidate
fi

# Validate the whole configuration with its load line pointing at the new file.
preview=$(mktemp "$config_dir/.disctui-preview.XXXXXXXX")
awk -v loader="$loader_pattern" -v staged="${staged_actions##*/}" '
	$0 ~ loader { sub(/disctui[.]lua/, staged) }
	{ print }
' "$proposed" >"$preview"
check_config "$preview"

mv -f "$staged_actions" "$actions"
printf 'Wrote %s (runs %s)\n' "$actions" "$bin"
if [[ -n $candidate ]]; then
	mv -f "$candidate" "$init_target"
	printf 'Added the load line to %s\n' "$init"
else
	printf '%s already loads it\n' "$init"
fi

if rex --autostart=false actions >/dev/null 2>&1; then
	rex --autostart=false config reload >/dev/null
	printf 'Reloaded Rex. Try: rex do disctui_jump mock=true\n'
else
	printf 'Rex is not running here; the actions load when it starts.\n'
fi

first=$(command -v disctui || true)
if [[ -n $first && $first != "$bin" ]] && ! has_rex_command "$first"; then
	printf 'Note: %s comes first on PATH and predates the Rex actions.\n' "$first"
	printf 'Rex uses %s; a shell still runs the older one.\n' "$bin"
fi
