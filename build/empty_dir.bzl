def _empty_dir_impl(ctx):
    d = ctx.actions.declare_directory(ctx.label.name)
    ctx.actions.run_shell(
        outputs = [d],
        command = "mkdir -p \"$1\"",
        arguments = [d.path],
    )
    return [DefaultInfo(files = depset([d]))]

empty_dir = rule(
    implementation = _empty_dir_impl,
    doc = "Declares an empty directory artifact.",
)
