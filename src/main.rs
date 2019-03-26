//! goto :: Flexible Working Directory Shortcuts
//!
//! Copyright (c) 2016-2017 by William R. Fraser

extern crate dirs;
extern crate docopt;
extern crate rustc_serialize;
extern crate toml;

use std::collections::btree_map::*;
use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use docopt::Docopt;

const CONFIG_FILENAME: &str = ".goto.toml";
const DEFAULT_SHELLCMD: &str = "pushd";

// 79 columns:
// ----------------------------------------------------------------------------
const USAGE: &str = r#"
Usage:
    goto [options] [<name> [<extra>]]
    goto --list
    goto (--help | --version)

Options:
    -c <command>, --cmd=<command>   # defaults to 'pushd'

Configuration is stored in ~/.goto.toml, with the following format:

    name = "/some/path"             # 'goto name' takes you here
    othername = "~/some/other/path" # $HOME expansion will happen

    ["/somewhere/specific"]         # Only in effect when in this location:
    "*" = "default/under/specific"  # With no arguments, this is used.
    "name" = "somewhere/else"       # Overshadows the one above.

Relative paths under a context header are resolved relative to the path in that
header. In the above example, when your current directory is under
/somewhere/specific, running 'goto name' takes you to
/somewhere/specific/somewhere/else.

If <extra> is provided as an extra argument, it is appended to the computed path.

goto is meant to be used as the argument to your shell's 'eval' builtin, like:
    function goto() {
        eval $(/usr/local/bin/goto $*)  # or wherever the 'goto' binary is
    }
"#;

#[derive(Debug, RustcDecodable)]
struct Args {
    arg_name: Option<String>,
    arg_extra: Option<String>,
    flag_cmd: Option<String>,
    flag_list: bool,
}

fn read_config_toml(config_path: &Path) -> io::Result<toml::Table> {
    let mut config_text = String::new();
    let mut file = File::open(config_path)?;
    file.read_to_string(&mut config_text)?;
    let mut parser = toml::Parser::new(&config_text);
    match parser.parse() {
        Some(config) => Ok(config),
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
            Err(io::Error::new(io::ErrorKind::Other, msg))
        }
    }
}

type PathMapping = BTreeMap<String, PathBuf>;

#[derive(Debug)]
struct Configuration {
    global: PathMapping,
    contexts: BTreeMap<PathBuf, PathMapping>,
}

fn parse_toml_as_path(t: &toml::Value, cwd: Option<&Path>) -> Result<PathBuf, String> {
    if let toml::Value::String(ref s) = *t {
        let path: PathBuf = if s.starts_with("~/") || s.starts_with("~\\") {
            env::home_dir().unwrap().join(&Path::new(&s[2..]))
        } else if cwd.is_some() {
            // note: this handles absolute paths correctly, i.e. by not using cwd at a all (except
            // for Windows, where the drive letter of cwd is considered.)
            cwd.unwrap().join(Path::new(&s))
        } else {
            return Err("expected an absolute path".into());
        };
        Ok(path)
    } else {
        Err(format!("type error: expected a string, not {}", t.type_str()))
    }
}

/// Process the parsed configuration TOML into goto's configuration struct.
/// if `cwd` is specified, all relative paths will be interpreted relative to that path.
fn process_config(config_toml: toml::Table, cwd: Option<&Path>) -> Result<Configuration, String> {
    let mut config = Configuration {
        global: PathMapping::new(),
        contexts: BTreeMap::new(),
    };

    for (k, v) in config_toml {
        if let toml::Value::Table(t) = v {
            // A path context.

            let context_path = match parse_toml_as_path(&toml::Value::String(k), cwd) {
                Ok(path) => path,
                Err(msg) => { return Err(format!("error: {}", msg)); }
            };

            let mut context_map = PathMapping::new();

            for (name, path) in t {
                let mapped_path: PathBuf = match parse_toml_as_path(&path, Some(&context_path)) {
                    Ok(path) => path,
                    Err(msg) => {
                        return Err(format!("error at {:?}.{}: {}", context_path, name, msg));
                    }
                };

                context_map.insert(name, mapped_path);
            }

            config.contexts.insert(context_path, context_map);
        } else {
            // A top-level entry. Attempt to parse as a path and insert into the global table.
            match parse_toml_as_path(&v, cwd) {
                Ok(path) => { config.global.insert(k, path); },
                Err(msg) => {
                    return Err(format!(
                        "error at {}: expected a table or a path string, not {} ({})",
                         k, v.type_str(), msg));
                },
            }
        }
    }

    Ok(config)
}

fn exit(msg: &str, fatal: bool) -> ! {
    io::stderr().write_all(msg.as_bytes()).unwrap();
    if !msg.ends_with('\n') {
        io::stderr().write_all(b"\n").unwrap();
    }
    let exit_code = if fatal { 1 } else { 0 };
    ::std::process::exit(exit_code);
}

fn print_path(path: &Path, shellcmd: &str, extra: &str) {
    if !shellcmd.is_empty() {
        print!("{} ", shellcmd);
    }

    // Because the path is potentially combined with the current working directory, which is
    // untrusted data, and the path is going to be evaluated by the shell, the path needs to be
    // single-quote escaped to prevent any expansion, for security.
    // (Otherwise a folder named '$(:(){:|:&};:)' would make for a bad day.)
    println!("'{}'", path.join(extra).to_str().unwrap().replace("'", "'\\''"));
}

fn main() {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| {
            d.version(Some(format!("goto {}", env!("CARGO_PKG_VERSION"))))
             .decode()
        })
        .unwrap_or_else(|e| {
            exit(&format!("{}", e), e.fatal());
        });

    let shellcmd = args.flag_cmd.as_ref().map(|s| s.as_str()).unwrap_or(DEFAULT_SHELLCMD);
    let name = args.arg_name.as_ref().map(|s| s.as_str()).unwrap_or("*");
    let extra = args.arg_extra.as_ref().map(|s| s.as_str()).unwrap_or("");

    let home = dirs::home_dir().unwrap_or_else(|| {
        exit("unable to determine home directory", true);
    });
    let config_path = home.join(Path::new(CONFIG_FILENAME));

    let cwd = env::current_dir().unwrap_or_else(|e| {
        exit(&format!("unable to get current working directory: {}", e), true);
    });

    let config = process_config(config_toml, Some(&home)).map_err(|msg| {
        exit(&format!("invalid configuration in {:?}: {}", config_path, msg), true);
    }).unwrap();

    // only used for the --list mode
    let mut effective_map = BTreeMap::<String, PathBuf>::new();

    // Contexts can have keys that overlap with other contexts. The rule is that the longest
    // context path that matches the CWD takes precedence.
    let mut done = false;
    let mut context_paths_by_len: Vec<&PathBuf> = config.contexts.keys().collect();
    context_paths_by_len.sort_by_key(|p| p.as_os_str().len());
    for context_path in context_paths_by_len.iter().rev() {
        if cwd.starts_with(context_path) {
            let map = &config.contexts[*context_path];
            if args.flag_list {
                for (k, v) in map {
                    if let Entry::Vacant(entry) = effective_map.entry(k.clone()) {
                        entry.insert(v.clone());
                    }
                }
            } else if let Some(path) = map.get(&*name) {
                print_path(path, shellcmd, extra);
                done = true;
                break;
            }
        }
    }

    if args.flag_list {
        for (k, v) in &config.global {
            if let Entry::Vacant(entry) = effective_map.entry(k.clone()) {
                entry.insert(v.clone());
            }
        }
        for (k, v) in effective_map {
            eprintln!("{} → {:?}", k, v);
        }
        done = true;
    }

    if !done {
        if let Some(path) = config.global.get(&*name) {
            print_path(path, shellcmd, extra);
            done = true;
        }
    }

    if !done {
        exit("not sure where to go", false);
    }
}
