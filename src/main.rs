extern crate docopt;
extern crate rustc_serialize;
extern crate toml;

use std::collections::btree_map::*;
use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use docopt::Docopt;

const USAGE: &'static str = "
Usage:
    goto [<name>]
    goto (--help | --version)

Configuration is stored in ~/.goto.toml

goto is meant to be used as the argument to your shell's 'eval' builtin, like so:
    function goto() {
        eval $(/usr/local/bin/goto $*)
    }
";

#[derive(Debug, RustcDecodable)]
struct Args {
    arg_name: Option<String>,
}

fn config_path() -> io::Result<PathBuf> {
    match env::home_dir() {
        Some(home) => Ok(home.join(".goto.toml")),
        None => Err(io::Error::new(io::ErrorKind::Other, "unable to determine home directory")),
    }
}

fn read_config() -> io::Result<toml::Table> {
    let mut config_text = String::new();
    let mut file = try!(File::open(try!(config_path())));
    try!(file.read_to_string(&mut config_text));
    let mut parser = toml::Parser::new(&config_text);
    let config = match parser.parse() {
        Some(v) => v,
        None => {
            let mut msg = String::from("failed to parse TOML:");
            for err in &parser.errors {
                let linecol_lo = parser.to_linecol(err.lo);
                let linecol_hi = parser.to_linecol(err.hi);
                msg.push_str(&format!("\n\t{} at {}:{} to {}:{}",
                        err.desc,
                        linecol_lo.0 + 1,
                        linecol_lo.1 + 1,
                        linecol_hi.0 + 1,
                        linecol_hi.1 + 1));
            }
            return Err(io::Error::new(io::ErrorKind::Other, msg));
        }
    };

    Ok(config)
}

type PathMapping = BTreeMap<String, PathBuf>;

#[derive(Debug)]
struct Configuration {
    global: PathMapping,
    contexts: BTreeMap<PathBuf, PathMapping>,
}

fn parse_toml_as_path(t: toml::Value, cwd: Option<&Path>) -> Result<PathBuf, String> {
    if let toml::Value::String(s) = t {
        let path: PathBuf = if s.starts_with("~/") {
            env::home_dir().unwrap().join(&Path::new(&s[2..]))
        } else if s.starts_with("/") {
            PathBuf::from(s)
        } else if cwd.is_some() {
            cwd.unwrap().join(Path::new(&s))
        } else {
            return Err(format!("expected a path starting with \"/\" or \"~/\""));
        };
        Ok(path)
    } else {
        Err(format!("type error: expected a string, not {}", t.type_str()))
    }
}

fn process_config(config_toml: toml::Table) -> Result<Configuration, String> {
    let mut config = Configuration {
        global: PathMapping::new(),
        contexts: BTreeMap::new(),
    };

    for (k, v) in config_toml.into_iter() {
        if let toml::Value::Table(t) = v {
            if k == "global" {
                for (name, path) in t.into_iter() {
                    match parse_toml_as_path(path, None) {
                        Ok(path) => { config.global.insert(name, path); },
                        Err(msg) => { return Err(format!("error at global.{}: {}", name, msg)); },
                    }
                }
            } else {
                let context_path = match parse_toml_as_path(toml::Value::String(k), None) {
                    Ok(path) => path,
                    Err(msg) => { return Err(format!("error: {}", msg)); }
                };

                let mut context_map = PathMapping::new();

                for (name, path) in t.into_iter() {
                    let mapped_path: PathBuf = match parse_toml_as_path(path, Some(&context_path)) {
                        Ok(path) => path,
                        Err(msg) => {
                            return Err(format!("error at {:?}.{}: {}", context_path, name, msg));
                        }
                    };

                    context_map.insert(name, mapped_path);
                }

                config.contexts.insert(context_path, context_map);
            }
        } else {
            return Err(format!("type error at {}: expected a table, not {}", k, v.type_str()));
        }
    }

    Ok(config)
}

fn main() {
    let args: Args = Docopt::new(USAGE)
                            .and_then(|d| {
                                d.version(Some(format!("goto {}", env!("CARGO_PKG_VERSION"))))
                                 .decode()
                            })
                            .unwrap_or_else(|e| {
                                writeln!(io::stderr(), "{}", e).unwrap();
                                ::std::process::exit(if e.fatal() { 1 } else { 0 });
                            });

    let name = args.arg_name.unwrap_or("*".to_owned());

    let cwd = PathBuf::from(env::current_dir().unwrap_or_else(|e| {
        panic!("unable to get current working directory: {}", e);
    }));

    let config_toml = read_config().map_err(|e| {
        panic!("failed to read configuration ~/.goto.toml: {}", e);
    }).unwrap();

    let config = process_config(config_toml).map_err(|msg| {
        panic!("invalid configuration in ~/.goto.toml: {}", msg);
    }).unwrap();

    let mut matched = false;
    let mut context_paths_by_len: Vec<&PathBuf> = config.contexts.keys().collect();
    context_paths_by_len.sort_by_key(|p| p.as_os_str().len());
    for context_path in context_paths_by_len.iter().rev() {
        if cwd.starts_with(context_path) {
            let map = config.contexts.get(*context_path).unwrap();
            if let Some(path) = map.get(&name) {
                println!("pushd {:?}", path);
                matched = true;
                break;
            }
        }
    }

    if !matched {
        if let Some(path) = config.global.get(&name) {
            println!("pushd {:?}", path);
            matched = true;
        }
    }

    if !matched {
        writeln!(io::stderr(), "not sure where to go").unwrap();
    }
}
