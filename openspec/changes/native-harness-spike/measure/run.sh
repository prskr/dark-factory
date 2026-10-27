#!/usr/bin/env bash
# Usage: run.sh <claude|df-harness|codex|idle> <run-no>
# One Podman container (= one cgroup) per run; memory.peak is read from inside it.
# Needs: DOCKER_HOST=unix:///run/user/501/podman/podman.sock, OMLX_API_KEY in env (df-harness, codex),
# $H with claude-acp/ (npm @agentclientprotocol/claude-agent-acp@0.95.0), claude (native CLI),
# df-harness (static musl, Bazel), codex/ (npm @agentclientprotocol/codex-acp), creds/ (a HOME with
# .claude/.credentials.json; never committed).
set -euo pipefail
who=$1 n=$2
here=$(cd "$(dirname "$0")" && pwd)
H=${H:?}; OUT=${OUT:?}/$who-$n; mkdir -p "$OUT"
work=$(mktemp -d); cp "$here"/task/{slug.js,test.js} "$work"/
chmod -R a+rwX "$work" "$OUT"
case $who in
  claude)     cmd=(node /h/claude-acp/node_modules/.bin/claude-agent-acp)
              env=(-e CLAUDE_CODE_EXECUTABLE=/h/claude -e HOME=/home/h -v "$H/creds:/home/h:Z") ;;
  df-harness) cmd=(/h/df-harness)
              env=(-e DF_HARNESS_BASE_URL=http://192.168.1.10:1234/v1 -e DF_HARNESS_API_KEY="$OMLX_API_KEY" -e DF_HARNESS_MODEL=qwen3.6-35b-a3b) ;;
  codex)      cmd=(node /h/codex/dist/index.js)
              env=(-e OMLX_API_KEY -e HOME=/home/h -e CODEX_HOME=/home/h/.codex -v "$H/codex-home:/home/h:Z") ;;
  idle)       cmd=(sleep 5); env=() ;;
esac
if [ "$who" = idle ]; then drv=(); else drv=(python3 -u "$here/drive.py" "$here/PROMPT.txt" /work); fi
"${drv[@]}" podman run -i --rm --user 0 \
  -v "$H:/h:ro,Z" -v "$work:/work:Z" -v "$OUT:/out:Z" -v "$here/sampler.js:/sampler.js:ro,Z" \
  -w /work "${env[@]}" docker.io/library/node:24-trixie-slim \
  node /sampler.js "${cmd[@]}" > "$OUT/drive.json" || true
[ "$who" = idle ] && exit 0
(cd "$work" && node test.js >/dev/null 2>&1 && echo solved > "$OUT/outcome" || echo failed > "$OUT/outcome")
cat "$OUT/drive.json" "$OUT/result.json" "$OUT/outcome"
