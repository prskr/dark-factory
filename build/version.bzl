"""Release version as an env file for rustc (`rustc_env_files`)."""

load("@bazel_skylib//rules:common_settings.bzl", "BuildSettingInfo")

def _version_env_impl(ctx):
    out = ctx.actions.declare_file(ctx.label.name + ".env")
    ctx.actions.write(out, "DF_VERSION=%s\n" % ctx.attr.version[BuildSettingInfo].value)
    return [DefaultInfo(files = depset([out]))]

version_env = rule(
    implementation = _version_env_impl,
    attrs = {"version": attr.label(mandatory = True, providers = [BuildSettingInfo])},
    doc = "Writes `DF_VERSION=<value of the version flag>`.",
)
