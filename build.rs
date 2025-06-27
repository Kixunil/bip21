use cargo_metadata::MetadataCommand;

fn main() {
    let meta = MetadataCommand::new().exec().unwrap();

    let pkg = meta.packages.iter().find(|p| *p.name == "bitcoin").unwrap();
    let ver = &pkg.version;

    println!("cargo:rustc-cfg=bitcoin_{}_{}", ver.major, ver.minor);
}
