use std::fs::File;
use std::io::{self, Read};
use std::path::PathBuf;
#[cfg(feature = "reqwest")]
use std::sync::Arc;

#[cfg(feature = "reqwest")]
use reqwest::{self, Client as HttpClient, Method, Request};
use quick_error::ResultExt;

use manifest::{FileData, Package, Packages, PackageData};
#[cfg(feature = "reqwest")]
use manifest::UrlData;
use recipients::Recipients;

const DEFAULT_EXPORT: &'static str = "export.rlib";

#[cfg(feature = "reqwest")]
quick_error! {
    #[derive(Debug)]
    pub enum DepotError {
        FileError(crate_name: String, source: FileData, err: io::Error) {
            context(context: (&'a str, FileData), err: io::Error) ->
                (context.0.to_owned(), context.1, err)
            description("file depot io error")
            display("File Depot failed to acquire '{}' from '{}' with I/O error: {}",
                crate_name, source.source.display(), err)
            cause(err)
        }
        TlsError(err: Arc<reqwest::Error>) {
            from()
            description("tls backend error")
            display("Failed to create TLS backend")
            cause(err.as_ref())
        }
        HttpError(crate_name: String, source: UrlData, err: reqwest::Error) {
            context(context: (&'a str, UrlData), err: reqwest::Error) ->
                (context.0.to_owned(), context.1, err)
            description("depot url error")
            display("Error parsing from url: {}", err)
            cause(err)
        }
        MissingLibraryFile(crate_name: String) {
            description("missing library file")
            display("No local library file to inject onto")
        }
        ArchiveError(err: ArchiveError) {
            from()
            description("missing library file")
            display("No local library file to inject onto")
            cause(err)
        }
    }
}

#[cfg(not(feature = "reqwest"))]
quick_error! {
    #[derive(Debug)]
    pub enum DepotError {
        FileError(crate_name: String, source: FileData, err: io::Error) {
            context(context: (&'a str, &'a FileData), err: io::Error) ->
                (context.0.to_owned(), context.1.clone(), err)
            description("file depot io error")
            display("File Depot failed to acquire '{}' from '{}' with I/O error: {}",
                crate_name, source.source.display(), err)
            cause(err)
        }
        MissingLibraryFile(crate_name: String) {
            description("missing library file")
            display("No local library file to inject onto")
        }
        ArchiveError(err: ArchiveError) {
            from()
            description("missing library file")
            display("No local library file to inject onto")
            cause(err)
        }
    }
}
quick_error! {
    #[derive(Debug)]
    pub enum ArchiveError {
        GzipError(crate_name: String, err: io::Error) {
            description("gzip io error")
            display("gzip failed to decode '{}' with I/O error: {}",
                crate_name, err)
            cause(err)
        }
        TarError(crate_name: String, err: io::Error) {
            description("tar io error")
            display("Tar failed to decode '{}' with I/O error: {}",
                crate_name, err)
            cause(err)
        }
        TarPathError(crate_name: String, err: io::Error) {
            description("tar path error")
            display("tar failed to decode path '{}' with I/O error: {}",
                crate_name, err)
            cause(err)
        }
        TarFileNameError(crate_name: String, path: PathBuf) {
            description("tar entry missing file name")
            display("Tar entry for '{}' did not have a file name in '{}'",
                crate_name, path.display())
        }
    }
}


#[derive(Debug)]
pub struct Depot {
    #[cfg(feature = "reqwest")]
    http_client: Result<HttpClient, Arc<reqwest::Error>>,
}

impl Depot {
    pub fn new() -> Self {
        Depot {
            #[cfg(feature = "reqwest")]
            http_client: HttpClient::new().map_err(Arc::new),
        }
    }

    pub fn deliver(&self, recipients: &Recipients, packages: Packages) -> Result<(), DepotError> {
        use self::DepotError::MissingLibraryFile;
        for (crate_name, package) in packages.packages.into_iter() {
            let dest = if let Some(dest) = recipients.get(crate_name.as_ref()) {
                dest
            } else {
                return Err(MissingLibraryFile(crate_name));
            };

            self.deliver_helper(crate_name, package, dest)?;
        }
        Ok(())
    }

    fn unpack<R: Read>(crate_name: String, r: R, dest: PathBuf) -> Result<(), ArchiveError> {
        use tar::Archive;
        use libflate::gzip::Decoder;
        use self::ArchiveError::*;

        // TODO add as configurable value
        let export_name = DEFAULT_EXPORT;

        let mut archive = Archive::new(Decoder::new(r).map_err(
            |e| GzipError(crate_name.clone(), e),
        )?);
        for entry in archive.entries().map_err(
            |e| TarError(crate_name.clone(), e),
        )?
        {
            let mut entry = entry.map_err(|e| TarError(crate_name.clone(), e))?;

            // If we have
            let new_dest;

            let dest = {
                let entry_path = entry.path().map_err(
                    |e| TarPathError(crate_name.clone(), e),
                )?;

                // TODO, validate path is only 1 level deep?

                let file_name = if let Some(file_name) = entry_path.file_name() {
                    file_name
                } else {
                    return Err(TarFileNameError(crate_name, entry_path.to_path_buf()));
                };

                if file_name == &*export_name {
                    dest.as_path()
                } else {
                    new_dest = dest.with_file_name(file_name);
                    new_dest.as_path()
                }
            };

            entry.unpack(dest).map_err(
                |e| TarError(crate_name.clone(), e),
            )?;
        }
        Ok(())
    }

    #[cfg(feature = "reqwest")]
    fn deliver_helper(
        &self,
        crate_name: String,
        package: Package,
        dest: PathBuf,
    ) -> Result<(), DepotError> {
        match package.data {
            PackageData::File(source) => {
                let source = File::open(&source.source).context((&*crate_name, source))?;
                Self::unpack(crate_name, source, dest)?;
            }
            PackageData::Url(source) => {
                let source = self.http_client
                    .as_ref()
                    .map_err(Arc::clone)?
                    .execute(Request::new(Method::Get, source.source.clone()))
                    .context((&*crate_name, source))?;
                Self::unpack(crate_name, source, dest)?;
            }
        }
        Ok(())
    }

    #[cfg(not(feature = "reqwest"))]
    fn deliver_helper(
        &self,
        crate_name: String,
        package: Package,
        dest: PathBuf,
    ) -> Result<(), DepotError> {
        match package.data {
            PackageData::File(source) => {
                let source = File::open(&source.source).context((&*crate_name, source))?;
                Self::unpack(crate_name, source, dest)?;
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::fs::{create_dir_all, File};
    use std::io::{Read, Write, Seek, SeekFrom, Error, Cursor};
    use std::path::Path;

    use libflate::gzip::Encoder;
    use tar::{Builder, Header};
    use tempdir::TempDir;

    use super::Depot;
    use recipients::Recipients;
    use manifest::{Packages, Package, PackageData, FileData};

    fn append_sized<W: Write, P: AsRef<Path>, R: AsRef<[u8]>>(
        builder: &mut Builder<W>,
        path: P,
        data: R,
    ) -> Result<(), Error> {
        let data = data.as_ref();
        let size = Cursor::new(data).seek(SeekFrom::End(0))?;

        let mut header = Header::new_old();
        header.set_path(path)?;
        header.set_size(size);
        header.set_cksum();

        builder.append(&header, data)
    }


    #[test]
    fn verify_file_delivery() {
        let base_dir = TempDir::new("example").unwrap();
        let private_dir = base_dir.path().join("private");
        let deps_dir = base_dir.path().join("deps");
        let out_dir = base_dir.path().join("build").join("example").join("out");
        create_dir_all(&out_dir).unwrap();
        create_dir_all(&deps_dir).unwrap();
        create_dir_all(&private_dir).unwrap();

        let dhltest_target = deps_dir.join("libdhltest-c000l0ff.rlib");
        let dhltest_dash_target = deps_dir.join("libdhltest_dash-d15ea5e.rlib");
        let dhltest_underscore_target = deps_dir.join("libdhltest_underscore-deadbeef.rlib");

        let dep1_name = "libbytes-f6610c9d61c318a7.rlib";
        let dep1_data = "test1";
        let dep2_name = "libcfg_if-8132ccc150e6610a.rlib";
        let dep2_data = "test2";
        let dep3_name = "libbyteorder-568dc38c19e619e7.rlib";
        let dep3_data = "test3";
        let dep4_name = "libforeign_types-ace5d92fe2c77261.rlib";
        let dep4_data = "test4";

        {
            File::create(&dhltest_target).unwrap();
            File::create(&dhltest_dash_target).unwrap();
            File::create(&dhltest_underscore_target).unwrap();
        }

        let dhltest_source = private_dir.join("dhltest.tar.gz");
        let dhltest_dash_source = private_dir.join("libdhltest_dash.tar.gz");
        let dhltest_underscore_source = private_dir.join("libdhltest_underscore.v12.tar.gz");

        {
            let file = File::create(&dhltest_source).unwrap();
            let gz = Encoder::new(file).unwrap();
            let mut tar = Builder::new(gz);
            append_sized(&mut tar, dep1_name, dep1_data).unwrap();
            append_sized(&mut tar, dep2_name, dep2_data).unwrap();
            append_sized(&mut tar, "export.rlib", "test5").unwrap();
            tar.into_inner().unwrap().finish().unwrap();

            let file = File::create(&dhltest_dash_source).unwrap();
            let gz = Encoder::new(file).unwrap();
            let mut tar = Builder::new(gz);
            append_sized(&mut tar, dep3_name, dep3_data).unwrap();
            append_sized(&mut tar, dep4_name, dep4_data).unwrap();
            append_sized(&mut tar, "export.rlib", "test6").unwrap();
            tar.into_inner().unwrap().finish().unwrap();

            let file = File::create(&dhltest_underscore_source).unwrap();
            let gz = Encoder::new(file).unwrap();
            let mut tar = Builder::new(gz);
            append_sized(&mut tar, "export.rlib", "test7").unwrap();
            tar.into_inner().unwrap().finish().unwrap();
        }

        let recipients = Recipients::with_env(&out_dir, base_dir.path()).unwrap();

        let mut packages = HashMap::new();
        packages.insert(
            "dhltest".into(),
            Package {
                data: PackageData::File(FileData { source: dhltest_source.clone() }),
                version: None,
            },
        );
        packages.insert(
            "dhltest-dash".into(),
            Package {
                data: PackageData::File(FileData { source: dhltest_dash_source.clone() }),
                version: None,
            },
        );
        packages.insert(
            "dhltest_underscore".into(),
            Package {
                data: PackageData::File(FileData { source: dhltest_underscore_source.clone() }),
                version: None,
            },
        );

        let depot = Depot::new();
        depot.deliver(&recipients, Packages { packages }).unwrap();

        {
            let mut s = String::new();

            s.clear();
            let mut file = File::open(&deps_dir.join(dep1_name)).unwrap();
            file.read_to_string(&mut s).unwrap();
            assert_eq!(s, dep1_data);

            s.clear();
            let mut file = File::open(&deps_dir.join(dep2_name)).unwrap();
            file.read_to_string(&mut s).unwrap();
            assert_eq!(s, dep2_data);

            s.clear();
            let mut file = File::open(&deps_dir.join(dep3_name)).unwrap();
            file.read_to_string(&mut s).unwrap();
            assert_eq!(s, dep3_data);

            s.clear();
            let mut file = File::open(&deps_dir.join(dep4_name)).unwrap();
            file.read_to_string(&mut s).unwrap();
            assert_eq!(s, dep4_data);



            s.clear();
            let mut file = File::open(&dhltest_target).unwrap();
            file.read_to_string(&mut s).unwrap();
            assert_eq!(s, "test5");

            s.clear();
            let mut file = File::open(&dhltest_dash_target).unwrap();
            file.read_to_string(&mut s).unwrap();
            assert_eq!(s, "test6");

            s.clear();
            let mut file = File::open(&dhltest_underscore_target).unwrap();
            file.read_to_string(&mut s).unwrap();
            assert_eq!(s, "test7");
        }

        base_dir.close().unwrap();
    }
}
