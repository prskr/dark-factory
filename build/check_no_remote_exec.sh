#!/usr/bin/env bash
# Every test that depends on df-test-postgres starts a container and must not run on a remote worker
# (`--config=rbe`, see docs/development.md): it needs the tag `no-remote-exec`. Fails and lists the targets
# that lack it. Usage: [BAZEL="bazel --bazelrc=.bazelrc.agents"] build/check_no_remote_exec.sh
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
read -ra bazel <<<"${BAZEL:-bazel}"

missing="$("${bazel[@]}" query \
	"kind(test, rdeps(//..., //crates/df-test-postgres:df_test_postgres)) except attr(tags, 'no-remote-exec', //...)" \
	2>/dev/null)"

if [[ -n "$missing" ]]; then
	echo "These tests depend on //crates/df-test-postgres but lack the tag \"no-remote-exec\":" >&2
	echo "$missing" >&2
	exit 1
fi
