#!/bin/sh
# AI Brain CLI — Installation
#
#   curl -fsSL https://raw.githubusercontent.com/peppermint-digital/ai-brain-cli/main/install.sh | sh
#
# Legt das Programm in ~/.local/bin ab und sorgt dafür, dass dieses
# Verzeichnis in zsh und bash im PATH liegt. Vorhandene Einträge werden
# erkannt, mehrfaches Ausführen ändert nichts doppelt.
#
# Bewusst POSIX-sh: das Skript läuft, bevor irgendetwas über die Shell des
# Nutzers bekannt ist.

set -eu

REPO_URL="https://github.com/peppermint-digital/ai-brain-cli.git"
BIN_NAME="ai-brain-cli"
INSTALL_DIR="${AI_BRAIN_COMPANION_BIN_DIR:-$HOME/.local/bin}"
MARKER="# ai-brain-cli: ~/.local/bin in den PATH"

say() { printf '%s\n' "$*"; }
warn() { printf '%s\n' "$*" >&2; }
die() { printf 'Fehler: %s\n' "$*" >&2; exit 1; }

need() {
    command -v "$1" >/dev/null 2>&1 || die "$2"
}

# --------------------------------------------------------------- fertig ----

# Welches gebaute Programm passt zu diesem Rechner?
ziel_erkennen() {
    system="$(uname -s)"
    arch="$(uname -m)"

    case "$system/$arch" in
        Linux/x86_64)          echo "x86_64-unknown-linux-gnu" ;;
        Darwin/arm64|Darwin/aarch64) echo "aarch64-apple-darwin" ;;
        Darwin/x86_64)         echo "x86_64-apple-darwin" ;;
        *)                     echo "" ;;
    esac
}

# Versucht, das gebaute Programm aus dem letzten Release zu holen.
#
# Gelingt das, spart es dem Nutzer eine Rust-Installation und eine halbe Minute
# Bauzeit. Gelingt es nicht — kein Release, unbekannte Plattform, kein Netz —
# faellt das Skript auf den Quelltext zurueck, statt abzubrechen.
download_release() {
    ziel="$(ziel_erkennen)"

    [ -n "$ziel" ] || return 1
    command -v curl >/dev/null 2>&1 || return 1

    say "Suche ein gebautes Programm fuer $ziel …"

    url="$(curl -fsSL "https://api.github.com/repos/peppermint-digital/ai-brain-cli/releases/latest" 2>/dev/null \
        | grep -o "https://[^\"]*ai-brain-cli-${ziel}" \
        | head -1)"

    [ -n "$url" ] || return 1

    work="$(mktemp -d)"
    trap 'rm -rf "$work"' EXIT INT TERM

    curl -fsSL "$url" -o "$work/ai-brain-cli" || return 1

    mkdir -p "$INSTALL_DIR"
    install -m 755 "$work/ai-brain-cli" "$INSTALL_DIR/$BIN_NAME"

    say "Fertiges Programm uebernommen — kein Bau noetig."
    return 0
}

# ---------------------------------------------------------------- bauen ----

build_from_source() {
    need git "git wird gebraucht, ist aber nicht installiert."
    need cargo "Rust (cargo) wird gebraucht. Installieren mit:
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
Danach eine neue Shell öffnen und dieses Skript erneut ausführen."

    work="$(mktemp -d)"
    # Auch bei Abbruch aufräumen — sonst bleiben Klone im temporären
    # Verzeichnis liegen, jeder ein paar hundert Megabyte nach dem Bauen.
    trap 'rm -rf "$work"' EXIT INT TERM

    say "Hole den Quelltext …"
    git clone --depth 1 --quiet "$REPO_URL" "$work/src"

    say "Baue (das dauert beim ersten Mal etwa eine halbe Minute) …"
    ( cd "$work/src" && cargo build --release --quiet )

    mkdir -p "$INSTALL_DIR"
    install -m 755 "$work/src/target/release/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
}

# ------------------------------------------------------------------ PATH ----

# Trägt die PATH-Zeile in eine Shell-Konfiguration ein — aber nur, wenn sie
# nicht schon drinsteht. Der Marker macht das Wiedererkennen unabhängig davon,
# wie die Zeile formatiert wurde.
ensure_path_in() {
    rc="$1"

    [ -e "$rc" ] || return 0

    if grep -Fq "$MARKER" "$rc" 2>/dev/null; then
        say "  $rc — Eintrag besteht bereits"
        return 0
    fi

    # Auch schon vorhandene, von Hand gesetzte Einträge respektieren, statt
    # eine zweite Zeile danebenzulegen.
    if grep -q '\.local/bin' "$rc" 2>/dev/null; then
        say "  $rc — enthält bereits einen Eintrag für ~/.local/bin, unverändert gelassen"
        return 0
    fi

    {
        printf '\n%s\n' "$MARKER"
        printf '%s\n' 'case ":$PATH:" in'
        printf '%s\n' '  *":$HOME/.local/bin:"*) ;;'
        printf '%s\n' '  *) PATH="$HOME/.local/bin:$PATH" ;;'
        printf '%s\n' 'esac'
    } >> "$rc"

    say "  $rc — Eintrag ergänzt"
    changed_rc=1
}

setup_path() {
    changed_rc=0

    case ":${PATH}:" in
        *":${INSTALL_DIR}:"*)
            say "PATH enthält ${INSTALL_DIR} bereits."
            return 0
            ;;
    esac

    say "Trage ${INSTALL_DIR} in die Shell-Konfiguration ein:"

    ensure_path_in "$HOME/.zshrc"
    ensure_path_in "$HOME/.bashrc"

    # Manche Anmeldungen lesen nur .bash_profile bzw. .profile. Nur anfassen,
    # wenn es die Datei schon gibt — sonst legen wir Nutzern Dateien an, die
    # ihre Anmeldung plötzlich anders ablaufen lassen.
    ensure_path_in "$HOME/.bash_profile"
    ensure_path_in "$HOME/.profile"

    if [ "$changed_rc" -eq 0 ]; then
        warn ""
        warn "Keine Shell-Konfiguration gefunden, die ich ergänzen konnte."
        warn "Füge diese Zeile selbst hinzu:"
        warn "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    fi
}

# ------------------------------------------------------------------ Lauf ----

say "AI Brain CLI — Installation"
say ""

if ! download_release; then
    say "Kein passendes Release gefunden — baue aus dem Quelltext."
    build_from_source
fi

say ""
setup_path

say ""
say "Installiert: $INSTALL_DIR/$BIN_NAME"
say ""

# Dieses Skript ist auch der Update-Weg. Wer schon verbunden ist, braucht KEIN
# `connect` — das legte einen zweiten Geraetezugang an: derselbe Rechner zweimal
# unter „Meine Geraete", der neue mit der Grundausstattung statt der gepflegten
# Auswahl, und die Zeiterfassung auf zwei Namen verteilt. Der Austausch des
# Programms laesst die Verbindung unberuehrt.
if [ -f "${XDG_CONFIG_HOME:-$HOME/.config}/ai-brain-cli/connection.json" ]; then
    say "Dieser Rechner ist bereits verbunden — kein 'connect' noetig."
    say ""
    say "Öffne eine neue Shell (oder:  . ~/.zshrc  bzw.  . ~/.bashrc), dann:"
    say ""
    say "  $BIN_NAME status"
    say "  $BIN_NAME sync --apply"
    say ""
else
    say "Öffne eine neue Shell (oder:  . ~/.zshrc  bzw.  . ~/.bashrc), dann:"
    say ""
    say "  $BIN_NAME connect deine-brain-domain.de"
    say ""
fi
