//! goto :: Flexible Working Directory Shortcuts
//!
//! Copyright (c) 2016-2024 by William R. Fraser

use std::collections::btree_map::*;
use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use docopt::Docopt;
use serde::Deserialize;

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

Configuration files can also be placed in any directory and will affect any
invocations of goto from that directory or below it. In the case of conflicts,
configurations from farther down the tree take precedence, and the one in your
home directory takes precedence over all others.

If <extra> is provided as an extra argument, it is appended to the computed path.

goto is meant to be used as the argument to your shell's 'eval' builtin, like:
    function goto() {
        eval $(/usr/local/bin/goto $*)  # or wherever the 'goto' binary is
    }
"#;

#[derive(Debug, Deserialize)]
struct Args {
    arg_name: Option<String>,
    arg_extra: Option<String>,
    flag_cmd: Option<String>,
    flag_list: bool,
}

fn read_config_toml(config_path: &Path) -> io::Result<toml::value::Table> {
    let mut config_text = String::new();
    let mut file = File::open(config_path)?;
    file.read_to_string(&mut config_text)?;
    match toml::from_str(&config_text) {
        Ok(config) => Ok(config),
        Err(e) => {
            Err(io::Error::new(io::ErrorKind::Other, format!("failed to parse TOML: {}", e)))
        }
    }
}

type PathMapping = BTreeMap<String, PathMappingEntry>;

#[derive(Debug, Clone)]
struct PathMappingEntry {
    dest: PathBuf,
    source_file: PathBuf,
}

#[derive(Debug, Default)]
struct Configuration {
    global: PathMapping,
    contexts: BTreeMap<PathBuf, PathMapping>,
}

/// Make the given TOML value into an absolute path. It should be a string, otherwise an error is
/// returned. If the path is relative, it is made absolute by interpreting it relative to the given
/// path, or to the user's home directory if it starts with "~/".
fn parse_toml_as_path(t: &toml::Value, relative_to: &Path) -> Result<PathBuf, String> {
    if let toml::Value::String(ref s) = *t {
        let path: PathBuf = if s.starts_with("~/") || s.starts_with("~\\") {
            dirs::home_dir().unwrap().join(Path::new(&s[2..]))
        } else {
            // note: this handles absolute paths correctly, by not using `relative_to` at all
            // (except for Windows, where the drive letter of `relative_to` may be considered).
            relative_to.join(Path::new(&s))
        };
        Ok(path)
    } else {
        Err(format!("type error: expected a string, not {}", t.type_str()))
    }
}

/// Process the parsed configuration TOML into goto's configuration struct.
/// All relative paths will be interpreted relative to `relative_to`.
fn process_config(config_file_path: &Path, config_toml: toml::value::Table, relative_to: &Path)
    -> Result<Configuration, String>
{
    let mut config = Configuration::default();

    for (k, v) in config_toml {
        if let toml::Value::Table(t) = v {
            // A path context.

            let context_path = match parse_toml_as_path(&toml::Value::String(k), relative_to) {
                Ok(path) => path,
                Err(msg) => { return Err(format!("error: {}", msg)); }
            };

            let mut context_map = PathMapping::new();

            for (name, path) in t {
                let mapped_path: PathBuf = match parse_toml_as_path(&path, &context_path) {
                    Ok(path) => path,
                    Err(msg) => {
                        return Err(format!("error at {:?}.{}: {}", context_path, name, msg));
                    }
                };

                context_map.insert(name, PathMappingEntry {
                    source_file: config_file_path.to_owned(),
                    dest: mapped_path,
                });
            }

            config.contexts.insert(context_path, context_map);
        } else {
            // A top-level entry. Attempt to parse as a path and insert into the global table.
            let mapped_path: PathBuf = match parse_toml_as_path(&v, relative_to) {
                Ok(path) => path,
                Err(msg) => {
                    return Err(format!(
                        "error at {}: expected a table or a path string, not {} ({})",
                         k, v.type_str(), msg));
                },
            };

            config.global.insert(k, PathMappingEntry {
                source_file: config_file_path.to_owned(),
                dest: mapped_path,
            });
        }
    }

    Ok(config)
}

/// Combine two configurations. The entries in `overlay` take precedence.
fn combine_configs(combined: &mut Configuration, mut overlay: Configuration) {
    combined.global.append(&mut overlay.global);
    for (context_path, mut context) in overlay.contexts {
        match combined.contexts.entry(context_path) {
            Entry::Occupied(mut combined_context) => {
                combined_context.get_mut().append(&mut context);
            },
            Entry::Vacant(entry) => {
                entry.insert(context);
            }
        }
    }
}

/// Read the configuration file at the given path.
/// If the file does not exist, returns Ok(None), otherwise if the file cannot be read or processed
/// for any reason, returns a message explaining the error.
fn read_config(config_path: &Path) -> Result<Option<Configuration>, String> {
    let config_toml = match read_config_toml(config_path) {
        Ok(toml) => toml,
        Err(ref e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("failed to read configuration {:?}: {}", config_path, e)),
    };

    process_config(config_path, config_toml, config_path.parent().unwrap())
        .map_err(|msg| {
            format!("invalid configuration in {:?}: {}", config_path, msg)
        })
        .map(Some)
}

/// Read and combine all configuration files for a given path, by walking up the directory stack
/// from the root to `cwd`, and finally the user's home configuration. If reading any of them
/// fails (other than because the file does not exist), returns an appropriate error message.
fn read_combine_configs(home_config_path: &Path, cwd: &Path) -> Result<Configuration, String> {
    assert!(cwd.is_absolute());

    let mut combined = Configuration::default();

    // Accumulate paths by stripping off components until we hit the root.
    let mut search_paths = Vec::<&Path>::new();
    let mut maybe_path = Some(cwd);
    while let Some(path) = maybe_path {
        search_paths.push(path);
        maybe_path = path.parent();
    }

    // Walk from the root up to `cwd`, reading and combining configs if they exist.
    for path in search_paths.iter().rev() {
        let toml_path = path.join(CONFIG_FILENAME);
        if let Some(config) = read_config(&toml_path)? {
            combine_configs(&mut combined, config);
        }
    }

    if let Some(config) = read_config(home_config_path)? {
        combine_configs(&mut combined, config);
    }

    Ok(combined)
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
    println!("'{}'", path.join(extra).to_str().unwrap().replace('\'', "'\\''"));
}

fn main() {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| {
            d.version(Some(format!("goto {}", env!("CARGO_PKG_VERSION"))))
             .deserialize()
        })
        .unwrap_or_else(|e| {
            exit(&format!("{}", e), e.fatal());
        });

    let shellcmd = args.flag_cmd.as_deref().unwrap_or(DEFAULT_SHELLCMD);
    let name = args.arg_name.as_deref().unwrap_or("*");
    let extra = args.arg_extra.as_deref().unwrap_or("");

    let home = dirs::home_dir().unwrap_or_else(|| {
        exit("unable to determine home directory", true);
    });
    let config_path = home.join(Path::new(CONFIG_FILENAME));

    let cwd = env::current_dir().unwrap_or_else(|e| {
        exit(&format!("unable to get current working directory: {}", e), true);
    });

    let config = read_combine_configs(&config_path, &cwd).unwrap_or_else(|msg| {
        exit(&msg, true);
    });

    // only used for the --list mode
    let mut effective_map = PathMapping::new();

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
            } else if let Some(entry) = map.get(name) {
                print_path(&entry.dest, shellcmd, extra);
                done = true;
                break;
            }
        }
    }

    if args.flag_list {
        for (k, v) in config.global {
            if let Entry::Vacant(entry) = effective_map.entry(k) {
                entry.insert(v);
            }
        }
        for (k, v) in effective_map {
            eprintln!("{} → {:?} (from {:?})", k, v.dest, v.source_file);
        }
        done = true;
    } else if !done {
        if let Some(entry) = config.global.get(name) {
            print_path(&entry.dest, shellcmd, extra);
            done = true;
        }
    }

    if !done {
        exit("not sure where to go", false);
    }
}
