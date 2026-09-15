fn main() {
    tonic_prost_build::compile_protos("proto/brain.proto").expect("compile proto/brain.proto");
}
