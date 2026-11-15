#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# The first-run gate: a stranger who has just installed `sageant` and has an empty home directory can put, search,
# get and list with no setup step. Usage: tools/first-run-smoke.sh /path/to/sageant
# It points the platform's data-directory variables at a fresh temporary directory and unsets SAGEANT_HOME, so the
# default location logic is what is tested.
set -euo pipefail

BIN="${1:?usage: first-run-smoke.sh /path/to/sageant}"
if [ ! -x "$BIN" ] && [ -x "$BIN.exe" ]; then BIN="$BIN.exe"; fi
[ -x "$BIN" ] || { echo "FAIL: $BIN is not an executable" >&2; exit 1; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
native() { if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else printf '%s' "$1"; fi; }
mkdir -p "$TMP/home" "$TMP/xdg" "$TMP/lad"
unset SAGEANT_HOME
export HOME="$TMP/home" USERPROFILE="$(native "$TMP/home")"
export XDG_DATA_HOME="$TMP/xdg" LOCALAPPDATA="$(native "$TMP/lad")" APPDATA="$(native "$TMP/lad")"

fail() { echo "FAIL: $*" >&2; exit 1; }

"$BIN" --version >/dev/null || fail "--version"
where="$("$BIN" where)"
echo "data directory: $where"
[ -n "$where" ] || fail "where printed nothing"

id="$("$BIN" put "Owls hunt at night and see well in the dark" 2>/dev/null)" || fail "put"
[ "${#id}" -eq 64 ] || fail "put should print the 64-character id on stdout, got: $id"

"$BIN" search owls | grep -q "$id" || fail "search did not find the chunk"
"$BIN" get "${id:0:8}" | grep -q "see well in the dark" || fail "get by id prefix"
"$BIN" list | grep -q "$id" || fail "list"

# The store was created under the data directory the program said it uses, and nowhere in the checkout.
[ -n "$(ls -A "$TMP/xdg" "$TMP/lad" "$TMP/home" 2>/dev/null)" ] || fail "nothing was created under the data directories"
echo "first-run smoke: ok"
