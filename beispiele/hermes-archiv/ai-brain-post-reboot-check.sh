#!/usr/bin/env bash
set -u

cd /home/codingmachine/Development/ai-brain || {
  echo "AI Brain Post-Reboot-Check: FEHLER - Projektpfad fehlt"
  exit 1
}

started_at=$(date '+%Y-%m-%d %H:%M:%S %Z')
fail=0

line() { printf '%-20s %s\n' "$1" "$2"; }
check_system() {
  local name="$1" status
  status=$(systemctl is-active "$name" 2>/dev/null || true)
  line "$name" "$status"
  [[ "$status" == "active" ]] || fail=1
}
check_user() {
  local name="$1" status
  status=$(systemctl --user is-active "$name" 2>/dev/null || true)
  line "$name" "$status"
  [[ "$status" == "active" ]] || fail=1
}

{
  echo "AI Brain Post-Reboot-Check — $started_at"
  echo
  check_system ai-brain
  check_system ai-brain-queue
  check_system reverb
  check_user nanoclaw
  check_user ai-brain-scheduler.timer

  if ss -ltn | grep -qE '(^|[[:space:]])LISTEN[[:space:]].*(:|\*)8001[[:space:]]'; then
    echo "web :8001 (frankenphp) ✓"
  else
    echo "web :8001 FEHLT ✗"
    fail=1
  fi

  public_code=$(curl -s -o /dev/null -w '%{http_code}' -m 10 https://brain.proxy.peppermint-digital.com/login || echo 000)
  echo "public -> $public_code"
  [[ "$public_code" == "200" ]] || fail=1

  avatar_tts_output=$(/home/codingmachine/.hermes/scripts/ai-brain-avatar-tts-watchdog.sh 2>&1 || true)
  if [[ -n "$avatar_tts_output" ]]; then
    echo "avatar/tts ✗"
    echo "$avatar_tts_output"
    fail=1
  else
    echo "avatar/tts ✓"
  fi

  echo
  if [[ "$fail" -eq 0 ]]; then
    echo "Ergebnis: OK — alles wie erwartet hochgekommen."
  else
    echo "Ergebnis: PROBLEM — mindestens ein Check ist fehlgeschlagen."
    echo
    echo "Hinweise:"
    echo "- user-Dienste fehlen: loginctl enable-linger codingmachine prüfen"
    echo "- public != 200 bei :8001 ok: NetBird/Tunnel/Proxy prüfen"
    echo "- Dashboard weiß: wahrscheinlich Browser-Cache, Strg+Shift+R"
  fi
}

exit "$fail"
