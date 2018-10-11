
extern crate protoc_rust;

use protoc_rust::Customize;
use std::env;
use std::fs;
use std::path::Path;
use std::io::Write;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("protos");
    if let Err(e) = fs::create_dir(&dest_path) {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            // We're OK
        }
        else {
            panic!("Failed to create destination path {:?}: {}", dest_path, e);
        }
    }
    let dest_path_str = dest_path.to_string_lossy().to_owned();

    protoc_rust::run(protoc_rust::Args {
        out_dir: &dest_path_str[..],
        input: &["control.proto", "device.proto"],
        includes: &[],
        customize: Customize {
            carllerche_bytes_for_bytes: Some(true),
            carllerche_bytes_for_string: Some(true),
            ..Default::default()
        },
    }).expect("protoc");

    // The only way I could find to load the message.rs correctly is to create another mod.rs with
    // a hardcoded path attribute. So create that file. This gets included from src/protos/mod.rs
    let dest_path = Path::new(&out_dir).join("protos/mod.rs");
    let control_path = Path::new(&out_dir).join("protos/control.rs");
    let device_path = Path::new(&out_dir).join("protos/device.rs");
    let mut f = fs::File::create(&dest_path).unwrap();

    writeln!(f, "#[path={:?}]", control_path).unwrap();
    writeln!(f, "pub mod control;").unwrap();
    writeln!(f, "#[path={:?}]", device_path).unwrap();
    writeln!(f, "pub mod device;").unwrap();
}
