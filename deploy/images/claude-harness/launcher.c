/*
 * Static launcher of the Claude harness image (openspec change harness-images, design D2).
 *
 * One binary, two names. It needs no shell and no libc of the project image: it runs the
 * bundled musl loader against the bundled libraries and starts
 *   claude-agent-acp  ->  Node.js running the ACP adapter
 *   claude            ->  the musl Claude Code CLI
 * The adapter is told to use the `claude` name through CLAUDE_CODE_EXECUTABLE.
 */
#include <libgen.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define ROOT "/opt/harness/claude-code"
#if defined(__aarch64__)
#define LOADER ROOT "/lib/ld-musl-aarch64.so.1"
#elif defined(__x86_64__)
#define LOADER ROOT "/lib/ld-musl-x86_64.so.1"
#else
#error "unsupported architecture"
#endif
#define SDK ROOT "/node_modules/@anthropic-ai/claude-agent-sdk-linux-"
#define ADAPTER ROOT "/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js"
#if defined(__aarch64__)
#define CLI SDK "arm64-musl/claude"
#else
#define CLI SDK "x64-musl/claude"
#endif

int main(int argc, char **argv) {
    char *self = strdup(argv[0]);
    if (self == NULL) {
        return 127;
    }
    int adapter = strcmp(basename(self), "claude") != 0;
    char *prefix[] = {LOADER, "--library-path", ROOT "/lib", adapter ? ROOT "/node" : CLI,
                      adapter ? ADAPTER : NULL};
    int n = adapter ? 5 : 4;
    char **args = calloc((size_t)(n + argc), sizeof(char *));
    if (args == NULL) {
        return 127;
    }
    memcpy(args, prefix, (size_t)n * sizeof(char *));
    for (int i = 1; i < argc; i++) {
        args[n + i - 1] = argv[i];
    }
    if (adapter) {
        setenv("CLAUDE_CODE_EXECUTABLE", ROOT "/bin/claude", 0);
    }
    execv(LOADER, args);
    perror("claude-harness launcher: exec " LOADER);
    return 127;
}
