# Dependency Hijacking Library

[Currently](https://github.com/rust-lang/cargo/issues/1139), Cargo only ships with support for dependencies that have source code available. DHL makes use of build scripts to allows linking against any dependencies, even closed source ones!

DHL works by using a local dummy crate, and then hijacking it with an arbitrary binary.

# Usage

Let's use DHL to link against the private library `priv:1.0.0`.

## Binary Setup

We start by placing the private previously compiled library. In the root of our project, we'll create a directory called `libs`. In that directory we'll create directories named after the triples of the targets we care about (e.g. `x86_64-pc-windows-msvc`). In each of those directories we'll create a directory for our rustc compiler version (e.g. `rustc-1.21.0-13d94d5fa`). In this directory we will place our `libpriv.rlib`.

## Dummy Setup

Now let's create the dummy. We create a folder called `priv` with a `Cargo.toml` as follows:

```toml
[package]
name = "priv"
version = "1.0.0"
authors = [""]
```

Within `priv`, we create a `src` folder with an empty `lib.rs` file.

## Injection

To setup our project `Cargo.toml` we need to do a few things:
* Add `priv` as both a `dependency` and a `build-dependency`
* Add `dhl` as a `build-dependency`
* Configure dhl metadata to find our previously compiled
* Add a build script to run dhl.

```toml
[package]
...
build = "build.rs"

[dependencies]
priv = { path = "priv" }

[build-dependencies]
priv = { path = "priv" }
dhl = "0.1.0"

[package.metadata.dhl.packages]
priv = "./libs/{{target}}/{{rustc_short_version}}/libpriv.lib"
```

`{{target}}` and `{{rustc_short_version}}` are by replaced during the build process via the handlebars templating engine.

As for our build script, it's pretty short:

```rust
extern crate dhl;

fn main() {
    dhl::simply_deliver();
}
```

And that's it. `cargo build` and we should be good.

# Options

Package options can be configured via `[package.metadata.dhl.packages]`. Each package can be assigned a source directly:

```toml
priv = "./libs/{{target}}/{{rustc_short_version}}/libpriv.lib"
```

or with additional options:

```toml
priv = { source = "./libs/{{target}}/{{rustc_short_version}}/libpriv.lib", link = "soft" }
```

Sources can either be a path to a file (relative paths are based on `CARGO_MANIFEST_DIR`), or a url. Currently the only supported schemes are:

* `file`
* `http`
* `https`

The only supported option is `link`, with valid parameters being `soft` and `hard`. If the link option is present either a symbolic link or a hard link to the library file is created. Otherwise the library file is copied. For obvious reasons, `link` is only supported for files.
