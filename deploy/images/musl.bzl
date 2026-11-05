"""Shared parts of the harness images: binaries patched to run from the bundled musl runtime
(design D6 of iter3-opencode-harness) and the runtime files of the image layer."""

_LOADER = select({
    "@platforms//cpu:aarch64": "LOADER=ld-musl-aarch64.so.1",
    "@platforms//cpu:x86_64": "LOADER=ld-musl-x86_64.so.1",
})

def patched_binary(name, srcs, prefix, out = None, member = None):
    """Copies a binary and sets its interpreter and RUNPATH to the musl runtime under `prefix`/lib.

    Args:
      name: target name.
      srcs: the (select of) source labels.
      prefix: where the image is mounted, e.g. `/opt/harness/opencode`.
      out: output file name, default `name`.
      member: file name to take from `srcs` when they are directories or several files (error
        when none is named so); default: `srcs` is the single binary.
    """
    pick = "SRC=$<" if member == None else (
        "SRC=; for d in $(SRCS); do if [ -f $$d/{m} ]; then SRC=$$d/{m}; fi; done; " +
        "if [ -z \"$$SRC\" ]; then echo 'no {m} in $(SRCS)' >&2; exit 1; fi"
    ).format(m = member)
    native.genrule(
        name = name,
        srcs = srcs,
        outs = [out or name],
        cmd = _LOADER + "\n" + pick + """
cp $$SRC $@
chmod u+w $@
$(execpath @multitool//tools/patchelf) \\
    --set-interpreter {prefix}/lib/$$LOADER \\
    --set-rpath {prefix}/lib $@
""".format(prefix = prefix),
        tools = ["@multitool//tools/patchelf"],
    )

def musl_runtime_files():
    """The loader and libraries of the musl runtime at `/lib` of the image layer (a select)."""
    return select({
        "@platforms//cpu:aarch64": {
            "/lib/ld-musl-aarch64.so.1": "@musl_runtime_linux_arm64//:lib/libc.so",
            "/lib/libgcc_s.so.1": "@musl_runtime_linux_arm64//:lib/libgcc_s.so.1",
            "/lib/libstdc++.so.6": "@musl_runtime_linux_arm64//:usr/lib/libstdc++.so.6",
        },
        "@platforms//cpu:x86_64": {
            "/lib/ld-musl-x86_64.so.1": "@musl_runtime_linux_amd64//:lib/libc.so",
            "/lib/libgcc_s.so.1": "@musl_runtime_linux_amd64//:lib/libgcc_s.so.1",
            "/lib/libstdc++.so.6": "@musl_runtime_linux_amd64//:usr/lib/libstdc++.so.6",
        },
    })
