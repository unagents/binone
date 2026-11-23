#!/bin/sh
# rustc wrapper (see config.toml beside this file): runs the compiler it is given with the
# build machine's directories replaced in everything the compiler records (panic locations,
# debug information). When several prefixes match a path, rustc uses the one given last, so the
# more specific ones come last.
rustc="$1"
shift
home="${HOME:-}"
cargo_home="${CARGO_HOME:-${home:+$home/.cargo}}"
rustup_home="${RUSTUP_HOME:-${home:+$home/.rustup}}"
set -- "$@" ${home:+--remap-path-prefix "$home=/home"}
set -- "$@" ${rustup_home:+--remap-path-prefix "$rustup_home=/rustup"}
set -- "$@" ${cargo_home:+--remap-path-prefix "$cargo_home=/cargo"}
exec "$rustc" "$@"
