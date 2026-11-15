{
  description = "Prepare new linked Git worktrees from a post-checkout hook";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: {
        default = pkgs.rustPlatform.buildRustPackage {
          pname = "git-wt";
          version = (pkgs.lib.importTOML ./Cargo.toml).package.version;
          src = self;
          cargoLock.lockFile = ./Cargo.lock;

          env.WT_DEFAULT_PYTHON = "${pkgs.python3}/bin/python3";
          doCheck = false;

          meta = {
            description = "Prepare new linked Git worktrees from a post-checkout hook";
            license = nixpkgs.lib.licenses.mit;
            mainProgram = "git-wt";
            platforms = systems;
          };
        };
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            clippy
            rustfmt
            python312
            python312Packages.pytest
            uv
            git
          ];
        };
      });
    };
}
