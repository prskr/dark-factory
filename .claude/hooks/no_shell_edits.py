#!/usr/bin/env python3
"""PreToolUse hook (Bash): block file edits through shell scripts.

AGENTS.md: change files with the edit tool, not with `sed -i`, `perl -pi`, `python3` heredocs or
`cat >`. Edits through the tool stay reviewable and fail loudly when the target text doesn't
match. Generated output (`cargo fmt`, lock files, code generators) is not matched here.

Writes whose targets are all under /tmp or /dev are allowed (scratch files, scripts that
analyse logs), as are `echo`/`printf` appends to the CI files ($GITHUB_OUTPUT, ...). Exit code 2
blocks the call and shows the reason to the agent. Copying a file prepared under /tmp into the
repo (`cp`/`mv`/`install`) counts as an edit. A heuristic, not a sandbox: it catches the usual
habits, not every way a shell can write a file (`git apply`, `awk … > f.new && mv` pass).
"""

import json
import re
import shlex
import sys

REASON = (
    "Blocked by .claude/hooks/no_shell_edits.py: change files with the Edit or Write tool, "
    "not with sed -i, perl -pi, python heredocs, tee or > redirects (AGENTS.md). Shell is fine "
    "for generated output (cargo fmt, lock files, code generators) and for files under /tmp."
)

IN_PLACE = re.compile(r"\b(sed|perl)\b([^|;&\n]*\s(-[a-zA-Z]*i[a-zA-Z]*\b|--in-place)[^|;&\n]*)")
# Options of sed and perl whose next argument is the script, not a file.
SCRIPT_OPTIONS = {
    "sed": ("-e", "-f", "--expression", "--file"),  # sed -E is extended regex, no argument
    "perl": ("-e", "-E"),
}
PY_WRITE = re.compile(
    r"\bpython3?\b.*?(open\(|\.write_text\(|\.write_bytes\(|shutil\.(copy|move)|fileinput)", re.S
)
# A writing mode argument of open(): positional or `mode=`, "w", "a", "x" or "r+".
PY_MODE = r"\s*,\s*(?:mode\s*=\s*)?['\"](?:[wax]|r\+|rb\+)"
# Write targets inside Python code, where they are literals.
PY_TARGETS = (
    re.compile(r"(?<![.\w])open\(\s*(['\"])([^'\"]+)\1" + PY_MODE),
    re.compile(
        r"Path\(\s*(['\"])([^'\"]+)\1\s*\)\.(?:write_(?:text|bytes)\(|open\(\s*(?:mode\s*=\s*)?['\"](?:[wax]|r\+))"
    ),
    re.compile(r"shutil\.(?:copy\w*|move)\([^,]+,\s*(['\"])([^'\"]+)\1"),
)
PY_OPAQUE_WRITE = re.compile(
    r"open\([^)]*" + PY_MODE + r"|\.write_(?:text|bytes)\(|shutil\.(?:copy\w*|move)\("
    r"|fileinput\.\w+\([^)]*inplace\s*=\s*(?:True|1)"
)
# A non-literal Python target is accepted if the script visibly works in a temporary directory.
PY_TMP_HINT = re.compile(r"/tmp/|TMPDIR|tempfile")
REDIRECT = re.compile(r"\b(cat|printf|echo)\b[^|;&\n]*?(?<![0-9&])>>?\s*(['\"]?)([^\s'\"]+)")
TEE = re.compile(r"\btee\b((?:\s+-\w+)*)((?:\s+[^\s|;&]+)*)")
# `cp`/`mv`/`install` in command position; renames and copies inside the repo are fine, a file
# prepared under /tmp and copied into the repo is an edit.
COPY = re.compile(r"(?:^|[;&|(]|\bthen|\bdo)\s*(?:sudo\s+)?(cp|mv|install)\s([^|;&\n]*)", re.M)
SAFE_TARGET = re.compile(
    r"^['\"]?(/tmp/|/dev/|&\d|\$\{?\w*(?i:tmp|temp|scratch)\w*)"
)
CI_FILE = re.compile(r"^['\"]?\$\{?GITHUB_(OUTPUT|ENV|PATH|STEP_SUMMARY)")
# Message arguments (git commit -m, gh pr create --body) may mention the blocked commands.
MESSAGE_ARG = re.compile(
    r"(?:\s-m|--message|--body|--title|\s-[fF]\s+\w+=|--(?:raw-)?field\s+\w+=)[\s=]*(['\"]).*?(?<!\\)\1",
    re.S,
)
# Quoted arguments with whitespace are literals (search patterns), not commands, unless they are
# a shell body (`bash -c '…'`) or contain a command substitution.
QUOTED_PHRASE = re.compile(r"(-c\s+)?('[^'\n]*\s[^'\n]*'|\"[^\"\n]*\s[^\"\n]*\")")


def literal(match: re.Match) -> str:
    executable = match.group(1) or "$(" in match.group(2) or "`" in match.group(2)
    return match.group(0) if executable else "''"


def blocked(command: str) -> bool:
    command = MESSAGE_ARG.sub(" ", command)
    if PY_WRITE.search(command) and python_writes_outside_tmp(command):
        return True
    shell = QUOTED_PHRASE.sub(literal, command)
    for match in IN_PLACE.finditer(shell):
        if in_place_outside_tmp(match.group(1), match.group(0)):
            return True
    for match in REDIRECT.finditer(shell):
        target = match.group(3)
        if match.group(1) in ("echo", "printf") and CI_FILE.match(target):
            continue
        if not SAFE_TARGET.match(target):
            return True
    for match in TEE.finditer(shell):
        if any(not SAFE_TARGET.match(target) for target in match.group(2).split()):
            return True
    # Paths with spaces are quoted: check copies on the command before literals are removed.
    return any(copies_tmp_into_repo(match.group(2)) for match in COPY.finditer(command))


def copies_tmp_into_repo(arguments: str) -> bool:
    """Whether `cp`/`mv`/`install` copies a file from /tmp to a target outside /tmp."""
    try:
        tokens = shlex.split(arguments)
    except ValueError:
        return False
    target_dir = None
    paths = []
    for index, token in enumerate(tokens):
        if token in ("-t", "--target-directory") and index + 1 < len(tokens):
            target_dir = tokens[index + 1]
        elif not token.startswith("-") and (index == 0 or tokens[index - 1] not in ("-t", "--target-directory")):
            paths.append(token)
    if target_dir is None and len(paths) >= 2:
        target_dir = paths.pop()
    if target_dir is None or SAFE_TARGET.match(target_dir):
        return False
    return any(SAFE_TARGET.match(source) for source in paths)


def in_place_outside_tmp(tool: str, segment: str) -> bool:
    """Whether an in-place `sed`/`perl` call edits a file outside /tmp and /dev."""
    try:
        tokens = shlex.split(segment.strip().strip("'\"$()`"))
    except ValueError:
        return True  # unparsable: be strict
    tokens = tokens[tokens.index(tool) + 1 :] if tool in tokens else tokens
    files, script_given, skip = [], False, False
    for token in tokens:
        if skip:
            skip = False
        elif token in SCRIPT_OPTIONS.get(tool, ()):
            script_given = skip = True
        elif token.startswith("-"):
            script_given = script_given or (tool == "perl" and "e" in token)
        else:
            files.append(token)
    if not script_given and files:
        files = files[1:]  # the first positional argument is the script
    return any(not SAFE_TARGET.match(path) for path in files)


def python_writes_outside_tmp(command: str) -> bool:
    targets = [m.group(2) for pattern in PY_TARGETS for m in pattern.finditer(command)]
    if any(not SAFE_TARGET.match(target) for target in targets):
        return True
    # A write whose target is not a literal: only allowed if the script works in a temp directory.
    opaque = len(PY_OPAQUE_WRITE.findall(command)) > len(targets)
    return opaque and not PY_TMP_HINT.search(command)


def main() -> int:
    try:
        payload = json.load(sys.stdin)
    except json.JSONDecodeError:
        return 0  # not our input: never block on a parse problem
    command = (payload.get("tool_input") or {}).get("command") or ""
    if blocked(command):
        print(REASON, file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
