{ pkgs, ... }:

{
  languages.rust = {
    enable = true;
    toolchainFile = ./rust-toolchain.toml;
  };

  packages = with pkgs; [
    pkg-config
    libpcap

    go-task
    lefthook

    cargo-watch
    cargo-nextest

    uv
  ];

  enterShell = ''
    echo "tylomon dev environment loaded"
    echo "Rust: $(rustc --version)"
    echo "Run 'task --list' for available commands"
    lefthook install
  '';
}
