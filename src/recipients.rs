use std::collections::HashMap;
use std::collections::hash_map::Entry::*;
use std::io::Result as IoResult;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::time::SystemTime;
use std::ffi::{OsString, OsStr};
use std::fs::Metadata;
use std::path::{Path, PathBuf};

use var_os_or;

quick_error! {
    #[derive(Debug)]
    pub enum RecipientsError {
        EnvError(name: &'static str) {
            description("environment variable error")
            display("Undefined environment variable '{}'", name)
        }
        InvalidOutDir(out_dir: OsString) {
            description("could not find deps dir from OUT_DIR")
            display("Could not find deps from using '{}'", Path::new(&out_dir).display())
        }
    }
}

/// The info locator stores on each library file
struct Address {
    file_name: OsString,
    last_modified: IoResult<SystemTime>,
    is_watched: AtomicBool,
}

impl Address {
    fn new(file_name: OsString, metadata: IoResult<Metadata>) -> Self {
        Address {
            file_name,
            last_modified: metadata.and_then(|v| v.modified()),
            is_watched: AtomicBool::new(false),
        }
    }
    fn is_newer(&self, other: &Self) -> bool {
        if let (Ok(a), Ok(b)) = (self.last_modified.as_ref(), other.last_modified.as_ref()) {
            a >= b
        } else {
            true
        }
    }

    /// Marks a library file as watched, returns true if this was the first
    /// watch.
    fn watch(&self) -> bool {
        !self.is_watched.load(Relaxed) && !self.is_watched.swap(true, Relaxed)
    }
}

pub struct Recipients {
    deps_dir: PathBuf,
    relative_deps_dir: Option<PathBuf>,
    addresses: HashMap<String, Address>,
}

impl Recipients {
    pub fn new() -> Result<Self, RecipientsError> {
        let out_dir = var_os_or("OUT_DIR", RecipientsError::EnvError)?;
        let manifest_dir = var_os_or("CARGO_MANIFEST_DIR", RecipientsError::EnvError)?;
        Self::with_env(&out_dir, &manifest_dir)
    }

    fn get_deps_dir<S>(out_dir: &S) -> Result<PathBuf, RecipientsError>
    where
        S: ?Sized + AsRef<OsStr>,
    {
        let out_dir = out_dir.as_ref();
        match Path::new(out_dir)
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent) {
            None => Err(RecipientsError::InvalidOutDir(out_dir.to_owned())),
            Some(d) => Ok(d.join(Path::new("deps"))),
        }
    }

    pub(super) fn with_env<S, T>(out_dir: &S, manifest_dir: &T) -> Result<Self, RecipientsError>
    where
        S: ?Sized + AsRef<OsStr>,
        T: ?Sized + AsRef<OsStr>,
    {
        let manifest_dir = manifest_dir.as_ref();
        let deps_dir = Self::get_deps_dir(out_dir.as_ref())?;
        Ok(Self::with_path(deps_dir, manifest_dir))
    }

    fn with_path<P>(deps_dir: PathBuf, manifest_dir: &P) -> Self
    where
        P: ?Sized + AsRef<Path>,
    {
        let relative_deps_dir = deps_dir.strip_prefix(manifest_dir).ok().map(PathBuf::from);

        let mut addresses = HashMap::new();
        for file in deps_dir.read_dir().unwrap() {
            let file = file.unwrap();
            let file_name = file.file_name();
            let utf_file_name: String = {
                // Skip entries that aren't utf8
                let utf_file_name = if let Some(file_name) = file_name.to_str() {
                    file_name
                } else {
                    continue;
                };
                // Skip entries that don't match libraries
                let utf_file_name =
                    if let Some(file_name) = utf_file_name.splitn(2, "lib").nth(1) {
                        file_name
                    } else {
                        continue;
                    };
                if !utf_file_name.ends_with(".rlib") {
                    continue;
                }
                // split on the -
                let utf_file_name = if let Some(file_name) = utf_file_name.splitn(2, '-').nth(0) {
                    file_name
                } else {
                    continue;
                };
                utf_file_name.into()
            };

            let info = Address::new(file_name, file.metadata());
            match addresses.entry(utf_file_name.clone()) {
                Vacant(entry) => {
                    entry.insert(info);
                }
                Occupied(mut entry) => {
                    if entry.get().is_newer(&info) {
                        println!(
                            "cargo:warning= duplicate entry for {}: '{}' ignored",
                            utf_file_name,
                            Path::new(&info.file_name).display(),
                        );
                    } else {
                        println!(
                            "cargo:warning= duplicate entry for {}: '{}' replaced with '{}'",
                            utf_file_name,
                            Path::new(&entry.get().file_name).display(),
                            Path::new(&info.file_name).display(),
                        );
                        entry.insert(info);
                    }
                }
            }
        }

        Recipients {
            deps_dir,
            relative_deps_dir,
            addresses,
        }
    }

    pub(super) fn get(&self, name: &str) -> Option<PathBuf> {
        let name = name.replace('-', "_");
        self.addresses.get(&name).map(|library| {
            let file_name = Path::new(&library.file_name);
            let dest = self.deps_dir.join(file_name);

            // Make sure we watch the library file for changes
            if library.watch() {
                let rel_dest = self.relative_deps_dir.as_ref().map(
                    |dir| dir.join(file_name),
                );
                let dest = rel_dest.as_ref().unwrap_or(&dest);
                // TODO: do we need to save this info?
                println!("cargo:rerun-if-changed={}", dest.display());
            }
            dest
        })
    }
}




#[cfg(test)]
mod test {
    use std::fs::{create_dir_all, File};
    use std::path::PathBuf;
    use std::ffi::OsStr;

    use tempdir::TempDir;

    use super::Recipients;

    #[test]
    fn check_deps_dir() {
        let base_dir = PathBuf::new().join("example");
        let deps_dir = base_dir.join("deps");
        let out_dir = base_dir.join("build").join("example").join("out");
        assert_eq!(
            Recipients::get_deps_dir::<OsStr>(out_dir.as_ref()).unwrap(),
            deps_dir
        );
    }

    include!(concat!(env!("OUT_DIR"), "/recipients.rs"));

    #[test]
    fn verify_tmp_deps() {
        let base_dir = TempDir::new("example").unwrap();
        let deps_dir = base_dir.path().join("deps");
        let out_dir = base_dir.path().join("build").join("example").join("out");
        create_dir_all(&out_dir).unwrap();
        create_dir_all(&deps_dir).unwrap();

        File::create(deps_dir.join("libdummy_dash-d15ea5e.rlib")).unwrap();
        File::create(deps_dir.join("libdummy_underscore-deadbeef.rlib")).unwrap();
        File::create(deps_dir.join("libdummy-c000l0ff.rlib")).unwrap();

        let r = Recipients::with_env(&out_dir, base_dir.path()).unwrap();

        r.get("dummy").unwrap();
        r.get("dummy-dash").unwrap();
        r.get("dummy_underscore").unwrap();

        base_dir.close().unwrap();
    }
}
