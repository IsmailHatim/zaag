#!/bin/sh
# Installs zaag on this machine. Safe to run again.
#
#   ./install.sh              the command, the status hooks, the skill, the zellij plugin and layout
#   ./install.sh --no-plugin  the command, the hooks and the skill only (a machine reached over SSH)
#   ./install.sh --build      compile the plugin (cargo + the wasm32-wasip1 target) instead of using dist/
set -eu
here=$(cd "$(dirname "$0")" && pwd)
bin="$HOME/.local/bin"
share="$HOME/.local/share/zaag"
wasm="$share/zaag-sidebar.wasm"
mode="${1:-}"

command -v python3 >/dev/null || { echo "zaag needs python3" >&2; exit 1; }
command -v zellij >/dev/null || [ -x "$bin/zellij" ] || echo "zaag: zellij is not installed yet (https://zellij.dev)" >&2

mkdir -p "$bin" "$share"
ln -sf "$here/bin/zaag" "$bin/zaag"
echo "zaag: command at $bin/zaag"
case ":$PATH:" in *":$bin:"*) ;; *) echo "zaag: add $bin to your PATH" >&2 ;; esac

"$bin/zaag" install-hooks >/dev/null
echo "zaag: Claude Code hooks in ~/.claude/settings.json"
if [ -d "$HOME/.codex" ]; then
  "$bin/zaag" install-hooks --agent codex >/dev/null
  echo "zaag: Codex hooks in ~/.codex/hooks.json (Codex asks once whether you trust them)"
fi
if [ -d "$HOME/.hermes" ]; then
  "$bin/zaag" install-hooks --agent hermes | grep -q merge_by_hand \
    && echo "zaag: ~/.hermes/config.yaml already has a hooks: block; run 'zaag install-hooks --agent hermes' and merge what it prints" \
    || echo "zaag: Hermes hooks in ~/.hermes/config.yaml (Hermes asks once whether you trust them)"
fi

# the skill, for every agent that reads SKILL.md folders
mkdir -p "$HOME/.claude/skills"
for skills in "$HOME/.claude/skills" "$HOME/.codex/skills" "$HOME/.hermes/skills" "$HOME/.agents/skills"; do
  [ -d "$skills" ] || continue
  ln -sfn "$here/skill" "$skills/zaag"
  echo "zaag: skill at $skills/zaag"
done

[ "$mode" = "--no-plugin" ] && exit 0

if [ "$mode" = "--build" ]; then
  "$here/build-plugin.sh"
fi
cp "$here/dist/zaag-sidebar.wasm" "$wasm"
echo "zaag: plugin at $wasm"

# the layout: every tab of an agents session has the sidebar on its left
side="            pane size=32 borderless=true {
                plugin location=\"file:$wasm\" {
                    mode \"sidebar\"
                    zaag \"$bin/zaag\"
                }
            }"
bar() { printf '        pane size=1 borderless=true {\n            plugin location="%s"\n        }\n' "$1"; }
config_dir="${ZELLIJ_CONFIG_DIR:-$HOME/.config/zellij}"
mkdir -p "$config_dir/layouts"
{
  echo "// Every tab gets the agents sidebar on its left (written by zaag's install.sh)."
  echo "layout {"
  for template in default_tab_template new_tab_template; do
    echo "    $template {"
    bar tab-bar
    echo '        pane split_direction="vertical" {'
    echo "$side"
    [ "$template" = default_tab_template ] && echo "            children" || echo "            pane"
    echo "        }"
    bar status-bar
    echo "    }"
  done
  printf '    tab {\n        pane\n    }\n}\n'
} > "$config_dir/layouts/zaag.kdl"
echo "zaag: layout at $config_dir/layouts/zaag.kdl"

# zellij asks before a plugin may read the sessions and run commands; answer for this one
case "$(uname)" in
  Darwin) cache="$HOME/Library/Caches/org.Zellij-Contributors.Zellij" ;;
  *) cache="${XDG_CACHE_HOME:-$HOME/.cache}/zellij" ;;
esac
mkdir -p "$cache"
grep -qs "\"$wasm\"" "$cache/permissions.kdl" || printf '"%s" {\n    ReadApplicationState\n    ChangeApplicationState\n    RunCommands\n}\n' "$wasm" >> "$cache/permissions.kdl"

# the key for the searchable list: Ctrl-o g (session mode)
config="$config_dir/config.kdl"
bind="        bind \"g\" {
            LaunchOrFocusPlugin \"file:$wasm\" {
                floating true
                move_to_focused_tab true
                mode \"picker\"
                zaag \"$bin/zaag\"
            }
            SwitchToMode \"normal\"
        }"
if [ ! -f "$config" ]; then
  printf 'keybinds {\n    session {\n%s\n    }\n}\n' "$bind" > "$config"
  echo "zaag: Ctrl-o g opens the agents list (new $config)"
elif grep -q "zaag-sidebar.wasm" "$config"; then
  echo "zaag: Ctrl-o g already set in $config"
elif grep -q '^    session {$' "$config"; then
  cp "$config" "$config.before-zaag"
  BIND="$bind" python3 - "$config" <<'PY'
import os, sys
path = sys.argv[1]
text = open(path).read()
marker = "    session {\n"
at = text.index(marker) + len(marker)
open(path, "w").write(text[:at] + os.environ["BIND"] + "\n" + text[at:])
PY
  echo "zaag: Ctrl-o g opens the agents list (added to $config, previous file kept as config.kdl.before-zaag)"
else
  echo "zaag: add this to the session mode of your keybinds in $config for the agents list:"
  echo "$bind"
fi

echo
echo "Open a project with:  zaag attach <project>"
