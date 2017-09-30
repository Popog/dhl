use std::fs::{copy, hard_link};
use std::io;
use std::path::{Path, PathBuf};
#[cfg(feature = "reqwest")]
use std::sync::Arc;
#[cfg(feature = "reqwest")]
use std::fs::File;

#[cfg(feature = "reqwest")]
use reqwest::{self, Client, Method, Request};
use symlink::symlink_file;
use quick_error::ResultExt;

use manifest::{FileData, Packages, PackageData};
#[cfg(feature = "reqwest")]
use manifest::UrlData;
use recipients::Recipients;


#[cfg(feature = "reqwest")]
quick_error! {
    #[derive(Debug)]
    pub enum DepotError {
        FileError(crate_name: String, err: FileError) {
            context(crate_name: &'a str, err: FileError) -> (crate_name.to_owned(), err)
            description("depot file error")
            display("File Depot failed to acquire '{}': {}", crate_name, err)
            cause(err)
        }
        TlsError(err: Arc<reqwest::Error>) {
            from()
            description("tls backend error")
            display("Failed to create TLS backend")
            cause(err.as_ref())
        }
        HttpError(crate_name: String, err: HttpError) {
            context(crate_name: &'a str, err: HttpError) -> (crate_name.to_owned(), err)
            description("depot url error")
            display("Error parsing from url: {}", err)
            cause(err)
        }
        MissingLibraryFile(crate_name: String) {
            description("missing library file")
            display("No local library file to inject onto")
        }
    }
}

#[cfg(not(feature = "reqwest"))]
quick_error! {
    #[derive(Debug)]
    pub enum DepotError {
        FileError(crate_name: String, err: FileError) {
            context(crate_name: &'a str, err: FileError) -> (crate_name.to_owned(), err)
            description("depot file error")
            display("File Depot failed to acquire '{}': {}", crate_name, err)
            cause(err)
        }
        MissingLibraryFile(crate_name: String) {
            description("missing library file")
            display("No local library file to inject onto")
        }
    }
}

quick_error! {
    #[derive(Debug)]
    pub enum FileError {
        Io(source: FileData, dest: PathBuf, err: io::Error) {
            context(context: (&'a FileData, &'a Path), err: io::Error) ->
                (context.0.clone(), context.1.to_owned(), err)
            description("io error")
            display("I/O error: {}", err)
            cause(err)
        }
    }
}

#[cfg(feature = "reqwest")]
quick_error! {
    #[derive(Debug)]
    pub enum HttpError {
        Io(source: UrlData, dest: PathBuf, err: io::Error) {
            context(context: (&'a UrlData, &'a Path), err: io::Error) ->
                (context.0.clone(), context.1.to_owned(), err)
            description("io error")
            display("I/O error: {}", err)
            cause(err)
        }
        Request(source: UrlData, dest: PathBuf, err: reqwest::Error) {
            context(context: (&'a UrlData, &'a Path), err: reqwest::Error) ->
                (context.0.clone(), context.1.to_owned(), err)
            description("tls backend error")
            display("Failed to create TLS backend")
            cause(err)
        }
    }
}

#[cfg(feature = "reqwest")]
#[derive(Debug)]
struct HttpClient(Client);


#[derive(Debug)]
pub struct Depot {
    #[cfg(feature = "reqwest")]
    http_client: Result<HttpClient, Arc<reqwest::Error>>,
}

#[cfg(feature = "reqwest")]
impl HttpClient {
    fn deliver(&self, source: &UrlData, dest: &Path) -> Result<(), HttpError> {
        let mut resp = self.0
            .execute(Request::new(Method::Get, source.source.clone()))
            .context((source, dest))?;

        let mut file = File::create(dest).context((source, dest))?;

        io::copy(&mut resp, &mut file).context((source, dest))?;

        Ok(())
    }
}


impl Depot {
    #[cfg(feature = "reqwest")]
    pub fn new() -> Self {
        Depot { http_client: Client::new().map(HttpClient).map_err(Arc::new) }
    }

    #[cfg(not(feature = "reqwest"))]
    pub fn new() -> Self {
        Depot {}
    }

    fn deliver_from_file(&self, source: &FileData, dest: &Path) -> Result<(), FileError> {
        use manifest::LinkOption::*;
        match source.link {
            None => {
                copy(&source.source, dest).context((source, dest))?;
            }
            Some(Soft) => symlink_file(&source.source, dest).context((source, dest))?,
            Some(Hard) => hard_link(&source.source, dest).context((source, dest))?,
        }
        Ok(())
    }

    #[cfg(feature = "reqwest")]
    pub fn deliver(&self, recipients: &Recipients, packages: Packages) -> Result<(), DepotError> {
        use self::DepotError::MissingLibraryFile;
        for (crate_name, package) in packages.packages.into_iter() {
            let dest = if let Some(dest) = recipients.get(crate_name.as_ref()) {
                dest
            } else {
                return Err(MissingLibraryFile(crate_name));
            };

            match &package.data {
                &PackageData::File(ref source) => {
                    self.deliver_from_file(&source, &dest).context(&*crate_name)?
                }
                &PackageData::Url(ref source) => {
                    self.http_client
                        .as_ref()
                        .map_err(Arc::clone)?
                        .deliver(source, &dest)
                        .context(&*crate_name)?
                }
            }
        }
        Ok(())
    }

    #[cfg(not(feature = "reqwest"))]
    pub fn deliver(&self, recipients: &Recipients, packages: Packages) -> Result<(), DepotError> {
        use self::DepotError::MissingLibraryFile;
        for (crate_name, package) in packages.packages.into_iter() {
            let dest = if let Some(dest) = get(recipients, crate_name.as_ref()) {
                dest
            } else {
                return Err(MissingLibraryFile(crate_name));
            };

            match &package.data {
                &PackageData::File(ref source) => {
                    self.deliver_from_file(&source, &dest).context(&*crate_name)?
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::fs::{create_dir_all, File};
    use std::io::{Read, Write};

    use tempdir::TempDir;

    use super::Depot;
    use recipients::Recipients;
    use manifest::{Packages, Package, PackageData, FileData};


    #[test]
    fn verify_file_delivery() {
        let base_dir = TempDir::new("example").unwrap();
        let private_dir = base_dir.path().join("private");
        let deps_dir = base_dir.path().join("deps");
        let out_dir = base_dir.path().join("build").join("example").join("out");
        create_dir_all(&out_dir).unwrap();
        create_dir_all(&deps_dir).unwrap();
        create_dir_all(&private_dir).unwrap();

        let dummy_target = deps_dir.join("libdummy-c000l0ff.rlib");
        let dummy_dash_target = deps_dir.join("libdummy_dash-d15ea5e.rlib");
        let dummy_underscore_target = deps_dir.join("libdummy_underscore-deadbeef.rlib");

        {
            File::create(&dummy_target).unwrap();
            File::create(&dummy_dash_target).unwrap();
            File::create(&dummy_underscore_target).unwrap();
        }

        let dummy_source = private_dir.join("dummy");
        let dummy_dash_source = private_dir.join("libdummy_dash.rlib");
        let dummy_underscore_source = private_dir.join("libdummy_underscore.v12.rlib");

        {
            let mut file = File::create(&dummy_source).unwrap();
            write!(file, "test1").unwrap();

            let mut file = File::create(&dummy_dash_source).unwrap();
            write!(file, "test2").unwrap();

            let mut file = File::create(&dummy_underscore_source).unwrap();
            write!(file, "test3").unwrap();
        }

        let recipients = Recipients::with_env(&out_dir, base_dir.path()).unwrap();

        let mut packages = HashMap::new();
        packages.insert(
            "dummy".into(),
            Package {
                data: PackageData::File(FileData {
                    source: dummy_source.clone(),
                    link: None,
                }),
                version: None,
            },
        );
        packages.insert(
            "dummy-dash".into(),
            Package {
                data: PackageData::File(FileData {
                    source: dummy_dash_source.clone(),
                    link: None,
                }),
                version: None,
            },
        );
        packages.insert(
            "dummy_underscore".into(),
            Package {
                data: PackageData::File(FileData {
                    source: dummy_underscore_source.clone(),
                    link: None,
                }),
                version: None,
            },
        );

        let depot = Depot::new();
        depot.deliver(&recipients, Packages { packages }).unwrap();

        {
            let mut s = String::new();
            let mut file = File::open(&dummy_target).unwrap();
            file.read_to_string(&mut s).unwrap();
            assert_eq!(s, "test1");

            s.clear();
            let mut file = File::open(&dummy_dash_target).unwrap();
            file.read_to_string(&mut s).unwrap();
            assert_eq!(s, "test2");

            s.clear();
            let mut file = File::open(&dummy_underscore_target).unwrap();
            file.read_to_string(&mut s).unwrap();
            assert_eq!(s, "test3");
        }

        base_dir.close().unwrap();
    }
}
