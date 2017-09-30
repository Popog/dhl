// build.rs

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("recipients.rs");
    let mut f = File::create(&dest_path).unwrap();

    write!(
        &mut f,
        r#"
#[test]
fn verify_deps() {{
    let r = Recipients::with_env("{out_dir}", "{manifest_dir}").unwrap();
    r.get("dummy").unwrap();
    r.get("dummy-dash").unwrap();
    r.get("dummy_underscore").unwrap();
}}"#,
        out_dir = out_dir,
        manifest_dir = manifest_dir,
    ).unwrap();
}
