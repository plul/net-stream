_list:
    @just --list --unsorted

# Check project
check:
    just --unstable --fmt --check
    nix fmt -- --check .
    taplo fmt --check `fd --extension=toml`
    prettier --check `fd --extension=md`
    cargo fmt -- --check 
    cargo clippy --tests --examples -- -D warnings
    taplo lint `fd --extension=toml`
    RUSTDOCFLAGS='-Dwarnings' cargo doc --no-deps
    cargo nextest run
    nix flake show
    cargo udeps
    cargo outdated --depth=1

# Format code
fmt:
    just --unstable --fmt
    nix fmt
    taplo fmt `fd --extension=toml`
    cargo fmt
    prettier --write `fd --extension=md`

# List tests
list-tests:
    cargo nextest list

# Show test groups
show-test-groups:
    cargo nextest show-config test-groups

# Run cargo check on changes
watch-check:
    watchexec --clear --restart --exts='rs,toml' -- cargo check --tests --examples

# List nightly features in use
list-nightly-features:
    rg '^#!\[feature(.*)\]'
