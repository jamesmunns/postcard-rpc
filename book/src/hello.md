# Hello, ov-twin!

## Cloning the repo

Let's start by cloning the project folder:

```sh
$ git clone https://github.com/OneVariable/ov-twin-fw
$ cd ov-twin-fw
```

All of the software we'll need for both the **Host** (your PC), and the **Target** (the RP2040) is
in the `source/` folder.

Let's move to the "workbook/firmware" project. Note that this is NOT a workspace, so you may need to
launch your editor here. We'll explain the other parts of the project later.

```sh
$ cd source/workbook/firmware
$ ls -lah
total 128
-rw-r--r--  1 james  staff    48K May  3 10:11 Cargo.lock
-rw-r--r--  1 james  staff   3.1K May  3 10:11 Cargo.toml
-rw-r--r--  1 james  staff   1.5K May  3 10:11 build.rs
-rw-r--r--  1 james  staff   678B May  3 10:11 memory.x
drwxr-xr-x  4 james  staff   128B May  3 10:11 src
```

## Build a project

We'll be building a project one at a time, from the `src/bin` folder. You can peek ahead if you'd
like, but there might be spoilers!

We'll start by building the first project, `hello-01`. This may take a bit if it's your first build,
or if the internet is a little slow:

```sh
$ cargo build --release --bin hello-01
   Compiling proc-macro2 v1.0.79
   Compiling unicode-ident v1.0.12
   Compiling syn v1.0.109
   Compiling version_check v0.9.4
   Compiling defmt v0.3.6
...
   Compiling fixed-macro-types v1.2.0
   Compiling fixed-macro v1.2.0
   Compiling pio-proc v0.2.2
    Finished release [optimized + debuginfo] target(s) in 16.44s
```

If you got an error, make sure you followed the [Setup steps](./setup.md), and let me know if you
are stuck!

We'll now work through all of the sensors on the board, so you can see how to interact with them.

We won't focus too much on how the drivers of these sensors were written, as that's outside the
scope of this workshop. Feel free to ask questions though!
