_PROTO_TOOLCHAIN = "@protobuf//bazel/private:proto_toolchain_type"

def _protoc_impl(ctx):
    protoc = ctx.toolchains[_PROTO_TOOLCHAIN].proto.proto_compiler.executable
    out = ctx.actions.declare_file(ctx.label.name)
    ctx.actions.symlink(output = out, target_file = protoc, is_executable = True)
    return [DefaultInfo(executable = out, files = depset([out]))]

protoc = rule(
    implementation = _protoc_impl,
    executable = True,
    toolchains = [_PROTO_TOOLCHAIN],
    doc = "protoc from the resolved proto toolchain: prebuilt with --prefer_prebuilt_protoc, so abseil/protobuf are not compiled.",
)
