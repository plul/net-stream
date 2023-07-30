{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = inputs @ {flake-parts, ...}:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = ["x86_64-linux"];

      perSystem = {
        config,
        pkgs,
        system,
        lib,
        self',
        ...
      }: {
        _module.args.pkgs = import inputs.nixpkgs {
          inherit system;
          overlays = [(import inputs.rust-overlay)];
        };

        formatter = pkgs.alejandra;

        packages = {
          rust-toolchain-stable = pkgs.rust-bin.stable.latest.minimal.override {
            extensions = ["rust-src" "clippy" "rust-analyzer"];
          };

          rust-toolchain-nightly-minimal = pkgs.rust-bin.selectLatestNightlyWith (toolchain: toolchain.minimal);

          rustfmt-nightly = pkgs.rust-bin.selectLatestNightlyWith (toolchain:
            toolchain.minimal.override {
              extensions = ["rustfmt"];
            });

          # Warp cargo udeps to use the nightly toolchain
          cargo-udeps = pkgs.stdenv.mkDerivation {
            name = "cargo-udeps";
            buildInputs = [pkgs.makeWrapper];
            buildCommand = ''
              mkdir -p $out/bin
              ln -s ${pkgs.cargo-udeps}/bin/cargo-udeps $out/bin/cargo-udeps-unwrapped
              wrapProgram $out/bin/cargo-udeps-unwrapped \
                --prefix PATH ":" "${config.packages.rust-toolchain-nightly-minimal}/bin"
              mv $out/bin/cargo-udeps-unwrapped $out/bin/cargo-udeps
            '';
          };
        };

        devShells.default = pkgs.mkShell {
          packages = [
            config.packages.rust-toolchain-stable
            config.packages.rustfmt-nightly
            config.formatter
            pkgs.taplo
            pkgs.just
            pkgs.fd
            pkgs.watchexec
            pkgs.cargo-nextest
            pkgs.cargo-outdated
            pkgs.cargo-audit
            config.packages.cargo-udeps
            pkgs.nodePackages.prettier
          ];
        };
      };
    };
}
