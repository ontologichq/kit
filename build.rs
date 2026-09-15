fn main() {
    println!("cargo:rerun-if-changed=proto/ontologic.proto");
    let files =
        protox::compile(["proto/ontologic.proto"], ["proto"]).expect("parse proto/ontologic.proto");
    tonic_prost_build::compile_fds(files).expect("generate the API from proto/ontologic.proto");
}
