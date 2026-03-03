{ pkgs, lib, config, inputs, ... }:

{
  packages = [
    pkgs.espflash
  ];

  languages.rust = {
    enable = true;
    channel = "nightly";
    version = "2025-09-01";
    components = [ "rustc" "rust-src" "cargo" "clippy" "rustfmt" "rust-analyzer" "miri" ];
    targets = [ "riscv32imac-unknown-none-elf" "x86_64-unknown-linux-gnu" ];
  };
}
