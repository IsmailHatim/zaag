#!/bin/sh
# Builds a throwaway zellij (own socket folder, own zaag state) with three projects and five
# stand-in agents, for recording demo.tape.   sh demo/setup.sh   then   sh demo/setup.sh --stop
#
# zellij keeps what it knows of past sessions in one place whatever the socket folder, so this
# refuses to run next to real sessions of the same names, and only ever touches these three.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=/tmp/zaag-demo
names="webapp api docs"
for n in $names; do
  if env -u ZELLIJ_SOCKET_DIR zellij list-sessions -n -s 2>/dev/null | grep -qx "$n" && [ ! -f "$root/.mine" ]; then
    echo "you already have a zellij session named $n: the demo would clash with it" >&2; exit 1
  fi
done
export ZELLIJ_SOCKET_DIR=/tmp/zaag-demo-sock ZAAG_STATE=$root/state ZAAG_MACHINES=" "
stop() {
  for n in $names; do
    zellij kill-session "$n" >/dev/null 2>&1 || true
    zellij delete-session "$n" >/dev/null 2>&1 || true
  done
}
[ -f "$root/.mine" ] && stop
rm -rf "$root" "$ZELLIJ_SOCKET_DIR"
[ "${1:-}" = "--stop" ] && exit 0
mkdir -p "$root" "$ZELLIJ_SOCKET_DIR"; : > "$root/.mine"
repo() { mkdir -p "$root/$1"; git -C "$root/$1" init -q -b "$2"; }
repo webapp feat/remember-me; repo api main; repo docs main
agent() { zaag new --session "$1" --cwd "$root/$1" --name "$2" ${5:+--split right} --cmd "sh $here/agent.sh $3 $4" >/dev/null; }
agent webapp Builder claude builder
agent webapp Reviewer codex reviewer split
agent api Tests claude tests
agent api Scratch codex idle split
agent docs Writer hermes writer
mkdir -p "$ZAAG_STATE" && echo '["webapp", "api", "docs"]' > "$ZAAG_STATE/order.json"
