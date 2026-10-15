#!/usr/bin/env bash
set -u

cd /home/codingmachine/Development/ai-brain || {
  echo "AI Brain Avatar/TTS Watchdog: Projektpfad fehlt"
  exit 1
}

fail=0
messages=()
add_problem() {
  messages+=("$1")
  fail=1
}

# Core runtime: the fix depends on queue + Reverb, not just the web process.
for svc in ai-brain.service ai-brain-queue.service reverb.service; do
  status=$(systemctl is-active "$svc" 2>/dev/null || true)
  [[ "$status" == "active" ]] || add_problem "$svc ist $status"
done

# Check the currently bound TTS provider health method when available.
tts_health=$(php artisan tinker --execute='
try {
    $tts = app(App\Contracts\Plugins\TextToSpeech::class);
    $class = get_class($tts);
    if (method_exists($tts, "isHealthy")) {
        echo $class." health=".($tts->isHealthy() ? "ok" : "fail")."\n";
    } else {
        echo $class." health=unknown\n";
    }
} catch (Throwable $e) {
    echo "error=".$e->getMessage()."\n";
}
' 2>&1 || true)

if ! grep -q 'health=ok' <<<"$tts_health"; then
  add_problem "TTS Health nicht OK: $tts_health"
fi

# Detect queue failures for the new voice-generation job.
failed_voice_jobs=$(php artisan tinker --execute='
try {
    if (! Schema::hasTable("failed_jobs")) { echo "0\n"; return; }
    echo DB::table("failed_jobs")
        ->where("payload", "like", "%GenerateAvatarVoice%")
        ->where("failed_at", ">=", now()->subDay())
        ->count()."\n";
} catch (Throwable $e) { echo "ERR:".$e->getMessage()."\n"; }
' 2>&1 || true)

if [[ ! "$failed_voice_jobs" =~ ^[0-9]+$ ]]; then
  add_problem "Failed-Job-Check nicht auswertbar: $failed_voice_jobs"
elif (( failed_voice_jobs > 0 )); then
  add_problem "GenerateAvatarVoice failed_jobs letzte 24h: $failed_voice_jobs"
fi

# Detect recent completed scheduled avatar messages that should have audio but do not.
missing_audio=$(php artisan tinker --execute='
use PeppermintDigital\AiBrainPluginAvatar\Models\AvatarMessage;
try {
    echo AvatarMessage::query()
        ->where("role", AvatarMessage::ROLE_AVATAR)
        ->where("is_pending", false)
        ->where("created_at", ">=", now()->subDay())
        ->where("created_at", "<=", now()->subMinutes(5))
        ->where(function ($query) {
            $query->whereNull("metadata->audio_url")
                ->orWhere("metadata->audio_url", "");
        })
        ->where("metadata->source", "scheduled_task")
        ->count()."\n";
} catch (Throwable $e) { echo "ERR:".$e->getMessage()."\n"; }
' 2>&1 || true)

if [[ ! "$missing_audio" =~ ^[0-9]+$ ]]; then
  add_problem "Avatar-Audio-DB-Check nicht auswertbar: $missing_audio"
elif (( missing_audio > 0 )); then
  add_problem "Scheduled Avatar Messages ohne audio_url letzte 24h: $missing_audio"
fi

# The original symptom was a PHP/Octane max-execution-time timeout during callback/TTS.
# Only look at the last hour so old fixed incidents do not keep paging.
cutoff=$(date -d '1 hour ago' '+%Y-%m-%d %H:%M:%S')
recent_timeouts=$(awk -v cutoff="$cutoff" '
  /^\[[0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2}:[0-9]{2}\]/ {
    ts=substr($0,2,19)
    if (ts >= cutoff && ($0 ~ /Maximum execution time|Avatar voice generation failed/)) count++
  }
  END { print count+0 }
' storage/logs/laravel.log 2>/dev/null)
if [[ "$recent_timeouts" =~ ^[0-9]+$ ]] && (( recent_timeouts > 0 )); then
  add_problem "Relevante Laravel Timeout/TTS-Fehler letzte Stunde: $recent_timeouts"
fi

if (( fail > 0 )); then
  echo "AI Brain Avatar/TTS Watchdog: PROBLEM"
  printf -- '- %s\n' "${messages[@]}"
fi

exit 0
