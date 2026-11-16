#!/bin/sh
# A stand-in for a coding agent, for the demo recording: it draws what an agent's screen could
# look like and reports its status to zaag the way a real agent's hooks do. The screen is drawn
# again whenever the pane changes size (a session nobody looks at yet has tiny panes).
kind=$1 role=$2
say() { [ "$1" = "$said" ] || { zaag hook --agent "$kind" --event "$1" </dev/null; said=$1; }; }
said=""
dim=$(printf '\033[2m') bold=$(printf '\033[1m') green=$(printf '\033[32m') yellow=$(printf '\033[33m') off=$(printf '\033[0m')
ask() { printf '%s> %s%s\n\n' "$bold" "$1" "$off"; }
step() { [ "$t" -ge "$1" ] && printf '%s  ● %s%s\n' "$dim" "$2" "$off"; }

draw() {  # $t: seconds since the start
  case "$role" in
    builder)
      ask 'Add "remember me" to the login form'
      step 5 "Reading src/auth/login.tsx"; step 7 "Reading src/auth/session.ts"
      step 8 "Editing src/auth/login.tsx"; step 10 "Editing src/auth/session.ts"; step 11 "Running npm test"
      if [ "$t" -ge 13 ]; then
        printf '\n%s  ✓ 42 tests passed%s\n\n  Added a "remember me" checkbox.\n  Ticked: the session lasts 30 days.\n' "$green" "$off"
      fi ;;
    reviewer)
      ask 'Review the diff on this branch'
      printf '  No blocking issues. Two remarks:\n   - session.ts: use SameSite=Lax\n   - login.tsx: label the checkbox\n' ;;
    tests)
      ask 'Fix the flaky rate-limit test'
      step 0 "Reading tests/rate_limit.test.ts"
      if [ "$t" -ge 8 ]; then
        printf '\n%s  The agent wants to run:%s\n\n    rm -rf .cache/ratelimit\n\n  Do you want to proceed?\n  > 1. Yes\n    2. Yes, and do not ask again\n    3. No\n' "$yellow" "$off"
      fi ;;
    writer)
      ask 'Document the new /sessions endpoint'
      step 0 "Reading api/sessions.py"; step 9 "Editing docs/reference/sessions.md"
      if [ "$t" -ge 17 ]; then printf '\n  docs/reference/sessions.md is written.\n'; fi ;;
    idle)
      printf '%s  Ready.%s\n' "$dim" "$off" ;;
  esac
}

status() {  # what the agent's hooks would report at $t
  case "$role" in
    builder) [ "$t" -ge 13 ] && say done || say working ;;
    reviewer) say working; say done ;;
    tests) [ "$t" -ge 8 ] && say blocked || say working ;;
    writer) [ "$t" -ge 17 ] && say done || say working ;;
  esac
}

zaag hook --agent "$kind" --event start </dev/null
start=$(date +%s) last=""
while :; do
  t=$(( $(date +%s) - start ))
  status
  now="$(stty size 2>/dev/null) $(draw | cksum)"
  if [ "$now" != "$last" ]; then
    printf '\033[H\033[2J\033[3J'; draw; last=$now
  fi
  [ "$t" -gt 600 ] && exit 0
  sleep 0.5
done
