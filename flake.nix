{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
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
          stable-rust-toolchain = pkgs.rust-bin.stable.latest.minimal.override {
            extensions = ["rust-src" "clippy"];
          };

          nightly-rust-toolchain = pkgs.rust-bin.selectLatestNightlyWith (toolchain:
            toolchain.minimal.override {
              extensions = ["rust-src" "clippy"];
            });

          nightly-rustfmt = pkgs.rust-bin.selectLatestNightlyWith (toolchain:
            toolchain.minimal.override {
              extensions = ["rustfmt"];
            });

          nightly-rust-analyzer = pkgs.rust-bin.selectLatestNightlyWith (toolchain:
            toolchain.minimal.override {
              extensions = ["rust-analyzer"];
            });

          # Warp cargo udeps to use the nightly toolchain
          cargo-udeps = pkgs.stdenv.mkDerivation {
            name = "cargo-udeps";
            buildInputs = [pkgs.makeWrapper];
            buildCommand = ''
              mkdir -p $out/bin
              ln -s ${pkgs.cargo-udeps}/bin/cargo-udeps $out/bin/cargo-udeps-unwrapped
              wrapProgram $out/bin/cargo-udeps-unwrapped \
                --prefix PATH ":" "${config.packages.nightly-rust-toolchain}/bin"
              mv $out/bin/cargo-udeps-unwrapped $out/bin/cargo-udeps
            '';
          };
        };

        devShells.default = pkgs.mkShell {
          packages = [
            config.packages.stable-rust-toolchain
            config.packages.nightly-rust-analyzer
            config.packages.nightly-rustfmt
            config.packages.cargo-udeps
            config.formatter
            pkgs.taplo
            pkgs.just
            pkgs.fd
            pkgs.watchexec
            pkgs.cargo-nextest
            pkgs.cargo-outdated
            pkgs.cargo-audit
            pkgs.nodePackages.prettier
          ];
        };
      };
    };
}
