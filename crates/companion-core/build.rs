fn main() {
    cc::Build::new()
        .include("../../firmware/shared/generated")
        .include("../../firmware/shared/vendor/nanopb")
        .define("PB_VALIDATE_UTF8", None)
        .file("../../firmware/shared/validate.c")
        .file("../../firmware/shared/generated/companion.pb.c")
        .file("../../firmware/shared/vendor/nanopb/pb_common.c")
        .file("../../firmware/shared/vendor/nanopb/pb_decode.c")
        .file("../../firmware/shared/vendor/nanopb/pb_encode.c")
        .compile("cafe_nanopb");
    println!("cargo:rerun-if-changed=../../firmware/shared");
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path().expect("bundled protoc"));
    config.compile_protos(&["../../protocol/companion.proto"], &["../../protocol"]).expect("protocol schema");
    println!("cargo:rerun-if-changed=../../protocol/companion.proto");
}
