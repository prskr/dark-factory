"""Builds a target with `--compilation_mode=opt`, whatever the command line says."""

def _opt_transition_impl(_settings, _attr):
    return {"//command_line_option:compilation_mode": "opt"}

_opt_transition = transition(
    implementation = _opt_transition_impl,
    inputs = [],
    outputs = ["//command_line_option:compilation_mode"],
)

def _optimized_impl(ctx):
    return [ctx.attr.src[0][DefaultInfo]]

optimized = rule(
    implementation = _optimized_impl,
    attrs = {
        "src": attr.label(cfg = _opt_transition, mandatory = True),
    },
    doc = "Forwards the files of `src`, built in opt mode (e.g. a plugin component that ships in an image).",
)
