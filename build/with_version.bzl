"""Runs a test built with `//build:version` set by a transition, whatever the command line sets."""

def _version_transition_impl(_settings, attr):
    return {"//build:version": attr.version}

_version_transition = transition(
    implementation = _version_transition_impl,
    inputs = [],
    outputs = ["//build:version"],
)

def _impl(ctx):
    test = ctx.attr.test[0]
    executable = ctx.actions.declare_file(ctx.label.name)
    ctx.actions.symlink(
        output = executable,
        target_file = test[DefaultInfo].files_to_run.executable,
        is_executable = True,
    )
    runfiles = ctx.runfiles(files = [executable]).merge(test[DefaultInfo].default_runfiles)

    # The fixed value the test compares against, independent of the flag.
    env = {
        "DF_EXPECTED_VERSION": ctx.attr.version,
        "DF_REQUIRE_EXPECTED_VERSION": "1",
    }
    inherited = {}
    if RunEnvironmentInfo in test:
        inherited = test[RunEnvironmentInfo].environment
    return [
        DefaultInfo(executable = executable, runfiles = runfiles),
        RunEnvironmentInfo(environment = dict(inherited, **env)),
    ]

with_version_test = rule(
    implementation = _impl,
    attrs = {
        "test": attr.label(mandatory = True, cfg = _version_transition, executable = True),
        "version": attr.string(mandatory = True),
    },
    test = True,
    doc = "Runs `test` built with `//build:version=<version>`; the test gets `DF_EXPECTED_VERSION=<version>`.",
)
