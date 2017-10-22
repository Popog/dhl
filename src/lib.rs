#[cfg(all(feature = "handlebars", feature = "rustc_version"))]
extern crate rustc_version;

#[cfg(feature = "handlebars")]
extern crate handlebars;

#[cfg(feature = "reqwest")]
extern crate reqwest;

#[macro_use]
extern crate serde_derive;
#[allow(unused_extern_crates)]
extern crate serde;
#[macro_use]
extern crate quick_error;
extern crate toml;
extern crate tar;
extern crate libflate;

#[cfg(test)]
extern crate tempdir;

use std::env::var_os;
use std::ffi::{OsStr, OsString};

mod depot;
mod recipients;
mod manifest;
#[cfg(feature = "handlebars")]
mod template;

pub use recipients::{Recipients, RecipientsError};
pub use manifest::{Manifest, Packages, ManifestCreationError, ManifestInspectionError};
pub use depot::{Depot, DepotError};


quick_error! {
    #[derive(Debug)]
    pub enum Error {
        RecipientsError(err: RecipientsError) {
            from()
            description("recipients error")
            display("failed to find recipients: {}", err)
            cause(err)
        }
        ManifestCreationError(err: ManifestCreationError) {
            from()
            description("manifest creation error")
            display("failed to create manifest: {}", err)
            cause(err)
        }
        ManifestInspectionError(err: ManifestInspectionError) {
            from()
            description("manifest load error")
            display("failed to inspect manifest: {}", err)
            cause(err)
        }
        DepotError(err: DepotError) {
            from()
            description("depot error")
            display("failed to deliver packages: {}", err)
            cause(err)
        }
    }
}

fn var_os_or<K: AsRef<OsStr>, E, F: FnOnce(K) -> E>(key: K, f: F) -> Result<OsString, E> {
    var_os(key.as_ref()).ok_or_else(|| f(key))
}

pub fn simply_deliver() -> Result<(), Error> {
    let depot = Depot::new();
    let manifest = Manifest::produce()?;
    let recipients = Recipients::new()?;
    let packages = manifest.inspect()?;
    depot.deliver(&recipients, packages)?;
    Ok(())
}
