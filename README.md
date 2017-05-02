# `goto` – Working Directory Shortcuts

You open a terminal window, and get to work. But first you need to change
directories. If you work on a large project, you probably end up doing that a
few times before you’re close to the thing you’re working on.


    $ cd projects/current_project/src/com/example/thing/components
    # aw shoot, what was it called again?
    $ ls
    ... lots of stuff ...
    $ cd frobnicator
    $ ls
    ... more stuff ...
    $ cd lib
    # etc.

Even if you remember exactly where it is in the tree you’re going, it’s a lot of
typing, right?

 `cd ~/projects/current_project/src/com/example/thing/components/frobnicator/lib`

A common workaround is to set up a handful of `alias`-es in your `.bashrc` or
other shell init script that go to commonly-used locations.

## Enter `goto`, a more flexible solution.

`goto` is a context-sensitive set of shortcuts to your working directories. By
context-sensitive, it means what it does depends on where your current directory
is. You can configure a default working subdirectory (and any number of named
alternates) for each of the projects you work on. Simply `cd` to anywhere in
those projects, and type `goto`, and it brings you to that working directory.

### Configuration

`goto` is configured by a simple plain text file in your home directory,
`~/.goto.toml`. It uses the TOML format to express structure in an easy-to-read
way. (TOML is a lot like the well-known “INI” format, but better defined and
more flexible.)

A sample:

    proj = "projects/current_project"
    dl = "downloads"
    pk = "packages"

    ["projects/current_project"]
    "*" = "src/com/example/thing/components/frobnicator/lib"
    comps = "src/com/example/thing/components"
    test = "tests/thing"

The top of the file defines a few named shortcuts. These are available from
everywhere; simply `goto <name>` to invoke them.

The line in brackets begins a context. It’s only active when you’re in a
subdirectory of the path given. It then defines some more shortcuts specific to
that context. The `*` shortcut is special: it is the default, used when you
don’t call `goto` with any arguments.

Paths are relative to your home directory, and the paths inside the context are
relative to the path of the context itself.

So in this case, a common flow might be:

    $ goto proj # or: cd projects/current_project
    $ goto comps
    $ ls
    $ cd <something>

A lot less typing.

You can also type `goto --list` to show the available shortcuts for your
current working directory.

### Advanced Configuration

Contexts can overlap too! `goto` matches contexts from the most precise one
first, out to the global context. A more-specific context can name shortcuts the
same as less-specific ones, and they will override them. So `goto test` could
bring you to project-wide UI tests in a project-wide context, but inside some
sub-component’s directory, it could be configured to bring you to that
component’s unit tests instead.

## Installation

Requirements:

1. rust compiler

To download, build, and install:

- `git clone https://github.com/wfraser/goto.git`
- `cd goto`
- `cargo build --release`
- `echo "function goto() { eval \$($(pwd)/target/release/goto \$*) }" >> ~/.bashrc`
- `. ~/.bashrc`

(adjust the last two lines as needed to suit your shell)

Note that `goto` is meant to be used with your shell’s `eval` function, because
that’s the only way to change your shell’s current directory. It prints
 `pushd <directory>`, which the shell must evaluate itself.

You can customize the command goto prints (instead of `pushd`) by passing a
`--cmd=<command>` argument.

Also note that unfortunately it's infeasible to support Windows' `cmd.exe` because
it lacks an `eval` facility, and its string quoting rules are really bizarre.
Powershell should work be easy to integrate with, though.

## Future Plans
1. Add some testing around the .goto.toml configuration parser :)
