use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

#[cfg(feature = "handlebars")]
use handlebars::TemplateRenderError;
#[cfg(feature = "reqwest")]
use reqwest::{Url, UrlError};
use toml::{self, de};
use quick_error::ResultExt;

use var_os_or;
#[cfg(feature = "handlebars")]
use template::{TemplateEngine, TemplateGenerationError};

quick_error! {
    #[derive(Debug)]
    pub enum ManifestCreationError {
        EnvError(name: &'static str) {
            from()
            description("environment variable error")
            display("Undefined environment variable '{}'", name)
        }
        Io(err: io::Error) {
            from()
            description("io error")
            display("I/O error: {}", err)
            cause(err)
        }
        Toml(err: de::Error) {
            from()
            description("toml error")
            display("TOML error: {}", err)
            cause(err)
        }
    }
}

#[cfg(all(feature = "handlebars", feature = "reqwest"))]
quick_error! {
    #[derive(Debug)]
    pub enum ManifestInspectionError {
        TemplateGeneration(err: TemplateGenerationError) {
            from()
            description("template failed to generate")
            display("Template generation error: {}", err)
            cause(err)
        }
        TemplateRender(crate_name: String, source: UninspectedPackage, err: TemplateRenderError) {
            context(context: (&'a str, &'a UninspectedPackage), err: TemplateRenderError) ->
                (context.0.to_owned(), context.1.clone(), err)
            description("crate source template failed to render")
            display("crate '{}' failed to render from '{:?}': {}", crate_name, source, err)
            cause(err)
        }
        Url(crate_name: String, source: UninspectedPackage, err: UrlError) {
            context(context: (&'a str, &'a UninspectedPackage), err: UrlError) ->
                (context.0.to_owned(), context.1.clone(), err)
            description("crate source url failed to parse")
            display("crate '{}' url failed to parse from '{:?}': {}", crate_name, source, err)
            cause(err)
        }
    }
}

#[cfg(all(not(feature = "handlebars"), feature = "reqwest"))]
quick_error! {
    #[derive(Debug)]
    pub enum ManifestInspectionError {
        #[cfg()]
        Url(crate_name: String, source: UninspectedPackage, err: UrlError) {
            context(context: (&'a str, &'a UninspectedPackage), err: UrlError) ->
                (context.0.to_owned(), context.1.clone(), err)
            description("crate source url failed to parse")
            display("crate '{}' url failed to parse from '{:?}': {}", crate_name, source, err)
            cause(err)
        }
    }
}
#[cfg(all(feature = "handlebars", not(feature = "reqwest")))]
quick_error! {
    #[derive(Debug)]
    pub enum ManifestInspectionError {
        TemplateGeneration(err: TemplateGenerationError) {
            from()
            description("template failed to generate")
            display("Template generation error: {}", err)
            cause(err)
        }
        TemplateRender(crate_name: String, source: UninspectedPackage, err: TemplateRenderError) {
            context(context: (&'a str, &'a UninspectedPackage), err: TemplateRenderError) ->
                (context.0.to_owned(), context.1.clone(), err)
            description("crate source template failed to render")
            display("crate '{}' failed to render from '{:?}': {}", crate_name, source, err)
            cause(err)
        }
    }
}
#[cfg(all(not(feature = "handlebars"), not(feature = "reqwest")))]
quick_error! {
    #[derive(Debug)]
    pub enum ManifestInspectionError {
    }
}

#[derive(Deserialize, Debug)]
struct Toml {
    package: TomlPackage,
    dependencies: HashMap<String, TomlDependency>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum TomlDependency {
    String(String),
    Table {
        version: Option<String>,
        path: Option<String>,
    },
}

#[derive(Deserialize, Debug)]
struct TomlPackage {
    metadata: TomlPackageMetadata,
}

#[derive(Deserialize, Debug)]
struct TomlPackageMetadata {
    dhl: TomlDhl,
}

#[cfg(feature = "handlebars")]
#[derive(Deserialize, Debug)]
struct TomlDhl {
    substitutions: Option<HashMap<String, TomlDhlSubstitution>>,
    packages: HashMap<String, String>,
}

#[cfg(feature = "handlebars")]
#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum TomlDhlSubstitution {
    String(String),
    Table {
        value: String,
        #[serde(default)]
        env: bool,
    },
}

#[cfg(not(feature = "handlebars"))]
#[derive(Deserialize, Debug)]
struct TomlDhl {
    packages: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub packages: HashMap<String, UninspectedPackage>,
    #[cfg(feature = "handlebars")]
    pub substitutions: HashMap<String, Substitution>,
    pub manifest_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct UninspectedPackage {
    pub version: Option<String>,
    pub source: String,
}

#[cfg(feature = "handlebars")]
#[derive(Debug, Clone)]
pub enum Substitution {
    Value(String),
    EnvironmentVariable(String),
}

#[derive(Debug, Clone)]
pub struct Packages {
    pub(super) packages: HashMap<String, Package>,
}

#[derive(Debug, Clone)]
pub struct Package {
    pub version: Option<String>,
    pub data: PackageData,
}

#[derive(Debug, Clone)]
pub enum PackageData {
    File(FileData),
    #[cfg(feature = "reqwest")]
    Url(UrlData),
}


#[derive(Debug, Clone)]
pub struct FileData {
    pub source: PathBuf,
}

#[cfg(feature = "reqwest")]
#[derive(Debug, Clone)]
pub struct UrlData {
    pub source: Url,
}

impl Manifest {
    // produce
    pub fn produce() -> Result<Self, ManifestCreationError> {
        let manifest_dir = PathBuf::from(var_os_or(
            "CARGO_MANIFEST_DIR",
            ManifestCreationError::EnvError,
        )?);
        let manifest_file = manifest_dir.join(Path::new("Cargo.toml"));
        Self::produce_from_file(manifest_dir, manifest_file)

    }

    fn produce_from_file(
        manifest_dir: PathBuf,
        manifest_file: PathBuf,
    ) -> Result<Self, ManifestCreationError> {
        let mut manifest_file = BufReader::new(File::open(manifest_file)?);
        let mut contents = String::new();
        manifest_file.read_to_string(&mut contents)?;
        Self::produce_from_string(manifest_dir, contents)
    }

    fn produce_from_string(
        manifest_dir: PathBuf,
        contents: String,
    ) -> Result<Self, ManifestCreationError> {
        Self::produce_from_toml(manifest_dir, toml::from_str::<Toml>(&*contents)?)
    }

    #[cfg(feature = "handlebars")]
    fn produce_from_toml(
        manifest_dir: PathBuf,
        contents: Toml,
    ) -> Result<Self, ManifestCreationError> {
        let Toml {
            package: TomlPackage {
                metadata: TomlPackageMetadata {
                    dhl: TomlDhl {
                        substitutions,
                        packages,
                    },
                },
            },
            dependencies,
        } = contents;

        let packages = Self::load_packages(packages, dependencies);

        let substitutions = match substitutions {
            Some(s) => {
                s.into_iter()
                    .map(|(k, v)| {
                        (
                            k,
                            match v {
                                TomlDhlSubstitution::String(v) => Substitution::Value(v),
                                TomlDhlSubstitution::Table {
                                    value: v,
                                    env: true,
                                } => Substitution::EnvironmentVariable(v),
                                TomlDhlSubstitution::Table {
                                    value: v,
                                    env: false,
                                } => Substitution::Value(v),
                            },
                        )
                    })
                    .collect()
            }
            None => {
                let mut map = HashMap::new();
                map.insert(
                    "target".to_owned(),
                    Substitution::EnvironmentVariable("TARGET".to_owned()),
                );
                map.insert(
                    "profile".to_owned(),
                    Substitution::EnvironmentVariable("PROFILE".to_owned()),
                );
                map
            }
        };

        Ok(Manifest {
            packages,
            substitutions,
            manifest_dir,
        })
    }

    #[cfg(not(feature = "handlebars"))]
    fn produce_from_toml(
        manifest_dir: PathBuf,
        contents: Toml,
    ) -> Result<Self, ManifestCreationError> {
        let Toml{
            package: TomlPackagePackage{
                metadata: TomlPackageMetadata{
                    dhl: TomlDhl{packages}
                }
            },
            dependencies,
        } = contents;

        let packages = Self::load_packages(packages, dependencies);

        Ok(Manifest {
            packages,
            manifest_dir,
        })
    }

    fn load_packages(
        packages: HashMap<String, String>,
        mut dependencies: HashMap<String, TomlDependency>,
    ) -> HashMap<String, UninspectedPackage> {
        packages
            .into_iter()
            .map(|(k, source)| {
                let version =
                    if let Some(TomlDependency::Table { version, .. }) = dependencies.remove(&k) {
                        version
                    } else {
                        None
                    };

                let v = UninspectedPackage { version, source };
                (k, v)
            })
            .collect()
    }

    #[cfg(feature = "handlebars")]
    pub fn inspect(self) -> Result<Packages, ManifestInspectionError> {
        let template = TemplateEngine::new(self.substitutions)?;
        let mut packages = HashMap::with_capacity(self.packages.len());
        for (crate_name, package) in self.packages.into_iter() {
            let source = {
                let source = package.source.as_ref();
                let version = package.version.as_ref().map(AsRef::as_ref);
                template.render(source, version).context((
                    crate_name.as_ref(),
                    &package,
                ))?
            };

            let data = Self::inspect_package_data_helper(
                self.manifest_dir.as_ref(),
                crate_name.as_ref(),
                &package,
                source.as_ref(),
            )?;

            packages.insert(
                crate_name,
                Package {
                    version: package.version,
                    data,
                },
            );
        }
        Ok(Packages { packages })
    }

    #[cfg(not(feature = "handlebars"))]
    pub fn inspect(self) -> Result<Packages, ParseError> {
        let mut packages = HashMap::with_capacity(self.packages.len());
        for (crate_name, package) in self.packages.into_iter() {
            let data = Self::inspect_package_data_helper(
                self.manifest_dir.as_ref(),
                crate_name.as_ref(),
                &package,
                package.source.as_ref(),
            )?;

            packages.insert(
                crate_name,
                Package {
                    version: package.version,
                    data,
                },
            );
        }
        Ok(Packages { packages })
    }

    #[cfg(feature = "reqwest")]
    fn inspect_package_data_helper(
        manifest_dir: &Path,
        crate_name: &str,
        package: &UninspectedPackage,
        source: &str,
    ) -> Result<PackageData, ManifestInspectionError> {
        // Start at the manifest dir and join. Absolute paths will just replace it.
        Ok(if source.starts_with("file://") {
            PackageData::File(FileData {
                source: manifest_dir.join(Path::new(source.split_at("file://".len()).1)),
            })
        } else if source.contains("://") {
            PackageData::Url(UrlData {
                source: Url::parse(source).context((crate_name, package))?,
            })
        } else {
            PackageData::File(FileData { source: manifest_dir.join(Path::new(source)) })
        })
    }

    #[cfg(not(feature = "reqwest"))]
    fn inspect_package_data_helper(
        manifest_dir: &Path,
        _crate_name: &str,
        package: &UninspectedPackage,
        source: &str,
    ) -> Result<PackageData, ManifestInspectionError> {
        // Start at the manifest dir and join. Absolute paths will just replace it.
        Ok(PackageData::File(FileData {
            source: manifest_dir.join(Path::new(if source.starts_with("file://") {
                source.split_at("file://".len()).1
            } else {
                source
            })),
        }))
    }
}


#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use toml;

    use super::{Toml, Manifest};

    const MANIFEST_1: &'static str = r#"
[package]
name = "test"
version = "1.0.0"
authors = [""]
build = "build.rs"

[dependencies]
foo = "0.4.5"
bar = { version = "^3.2", optional = true }
priv = { path = "priv" }

[build-dependencies]
priv = { path = "priv" }

[package.metadata.dhl.packages]
priv = "file://lib/libpriv.tar.gz"
priv2 = "./lib/libpriv2.tar.gz"
priv3 = "http://example.com/libpriv.tar.gz"
"#;

    #[test]
    fn parse_toml() {
        toml::from_str::<Toml>(MANIFEST_1).unwrap();
    }

    #[test]
    fn simple_manifest() {
        Manifest::produce_from_string(PathBuf::new(), MANIFEST_1.into()).unwrap();
    }

}
