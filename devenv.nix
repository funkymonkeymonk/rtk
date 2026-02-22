# Minimal devenv for RTK development
{ pkgs, lib, config, inputs, ... }:

{
  packages = [
    pkgs.git
    pkgs.jujutsu
    pkgs.cargo-watch
    pkgs.cargo-nextest
    # Use plain rustc/cargo from nixpkgs instead of languages.rust
    pkgs.rustc
    pkgs.cargo
    pkgs.clippy
    pkgs.rustfmt
  ];

  scripts = {
    check.exec = "cargo fmt --all --check && cargo clippy --all-targets && cargo test";
    dev.exec = "cargo watch -x check -x test";
  };
}
