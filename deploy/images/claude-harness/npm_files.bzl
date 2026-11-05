"""Leaves npm packages out of a `rules_js` package tree."""

def _impl(ctx):
    # A package is a directory `@scope/name` below `node_modules` and, in the rules_js store, a
    # directory `@scope+name@version`; match both as whole path segments.
    segments = []
    for prefix in ctx.attr.package_prefixes:
        segments += ["/" + prefix, "/" + prefix.replace("/", "+")]
    kept = [
        f
        for f in ctx.attr.src[DefaultInfo].files.to_list()
        if not any([segment in f.path for segment in segments])
    ]
    return [DefaultInfo(files = depset(kept))]

npm_files_without_packages = rule(
    implementation = _impl,
    attrs = {
        "package_prefixes": attr.string_list(
            mandatory = True,
            doc = "Package names (`@scope/name`) or name prefixes to leave out.",
        ),
        "src": attr.label(mandatory = True),
    },
    doc = "The files of the npm package target `src` except those of the named packages.",
)
