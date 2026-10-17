{
  description = "carrel: a research paper library for you and your AI";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      eachSystem = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = eachSystem (pkgs: rec {
        carrel = pkgs.callPackage ./nix/package.nix { };
        default = carrel;
      });

      devShells = eachSystem (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.carrel ];
          packages = with pkgs; [
            clippy
            rustfmt
            rust-analyzer
            mdbook # the docs
            uv # the default extractor outside of Nix builds
          ];
        };
      });

      overlays.default = final: _prev: { carrel = final.callPackage ./nix/package.nix { }; };

      formatter = eachSystem (pkgs: pkgs.nixfmt);
    };
}
