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
        default = pkgs.stdenvNoCC.mkDerivation {
          pname = "git-worktree-init";
          version = "0.1.0";
          src = self;

          installPhase = ''
            mkdir -p $out
            cp -r hook.sh wti.py git_worktree_init bin $out/
            substituteInPlace $out/hook.sh $out/bin/git-wt \
              --replace-fail '"''${python:-python3}"' '"''${python:-${pkgs.python3.interpreter}}"'
          '';

          passthru.python = pkgs.python3;

          meta = {
            description = "Prepare new linked Git worktrees from a post-checkout hook";
            mainProgram = "git-wt";
            platforms = systems;
          };
        };
      });
    };
}
