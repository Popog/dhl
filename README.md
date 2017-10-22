# Dependency Hijacking Library

[Currently](https://github.com/rust-lang/cargo/issues/1139), Cargo only ships with support for dependencies that have source code available. DHL makes use of build scripts to allows linking against any dependencies, even closed source ones!

DHL works by using a local dummy crate, and then hijacking it with an arbitrary binary.

## Example

Let's use DHL to link against the private library `priv:1.0.0`.

### Export

To create an export from a project, first run `cargo clean`. Then run `cargo build` with whatever configuration and profile settings you want. Rename the `target\<profile>\lib<file>.rlib` to `export.rlib` and add it to a `exported.tar.gz` along with all the the `.rlib` files from the `deps` folder. It will probably look something like:

```
exported.tar.gz
└── exported.tar
    ├── export.rlib
    ├── libdhltest_dash-cd91f6fd9f58022a.rlib
    ├── libdhltest_underscore-b5b654186491c38a.rlib
    └── libdhltest-6d5270055f165b9c.rlib
```

### Import

#### Binary Setup

We start by acquiring the private previously compiled library. In the root of our project, we'll create a directory called `libs`. In that directory we'll create directories named after the triples of the targets we care about (e.g. `x86_64-pc-windows-msvc`). In each of those directories we'll create a directory for our rustc compiler version (e.g. `rustc-1.21.0-13d94d5fa`). In this directory we will place our `exported.tar.gz`.

#### Dummy Setup

Now let's create the dummy. We create a folder called `priv` with a `Cargo.toml` as follows:

```toml
[package]
name = "priv"
version = "1.0.0"
authors = [""]
```

Within `priv`, we create a `src` folder with an empty `lib.rs` file.

#### Injection

To setup our project `Cargo.toml` we need to do a few things:
* Add `priv` to `dependencies` and a `build-dependencies`
* Add any `dependencies` from `priv`.
* Add `dhl` as a `build-dependencies`
* Configure dhl metadata to find our previously compiled
* Add a build script to run dhl.

```toml
[package]
...
build = "build.rs"

[dependencies]
priv = { path = "priv" }
...

[build-dependencies]
priv = { path = "priv" }
dhl = "^0.1"

[package.metadata.dhl.packages]
priv = "./libs/{{target}}/{{rustc_short_version}}/exported.tar.gz"
```

`{{target}}` and `{{rustc_short_version}}` are by replaced during the build process via the handlebars templating engine.

As for our build script, it's pretty short:

```rust
extern crate dhl;

fn main() {
    dhl::simply_deliver().unwrap();
}
```

And that's it. `cargo build` and we should be good.

## Options

Package options can be configured via `[package.metadata.dhl.packages]`. Each package can be assigned a source directly:

```toml
priv = "./libs/{{target}}/{{rustc_short_version}}/exported.tar.gz"
```

Sources can either be a path to a file (relative paths are based on `CARGO_MANIFEST_DIR`), or a url. Currently the only supported schemes are:

* `file`
* `http`
* `https`

As for the substitutions, the built-ins available are:

* `{{rustc_short_version}}`
* `{{target}}`
* `{{profile}}`

The last two take their values from environment variables. These can be removed or changed by providing a `[package.metadata.dhl.substitutions]` section. These can either be a direct string assignment:

```toml
foo = "bar"
```

or using the value from an environment variable:

```toml
foo = { value = "BAR", env = true }
```

## FAQ

### Can my code be reverse engineered from .rlib files

Yes. AFAIK, no effort has been put in to obfuscate .rlib files. In theory you could run your source code through an obfuscator and distribute that. I'm not aware of any obfuscators for rust, but if anyone knows of one I'd be happy to link it here.

