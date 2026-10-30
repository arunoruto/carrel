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
            nodejs # the web UI: npm ci && npm run build in web/
            uv # the default extractor outside of Nix builds
          ];
        };
      });

      overlays.default = final: _prev: { carrel = final.callPackage ./nix/package.nix { }; };

      nixosModules.default =
        { lib, pkgs, ... }:
        {
          imports = [ ./nix/module.nix ];
          services.carrel.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.carrel;
        };

      homeManagerModules.default =
        { lib, pkgs, ... }:
        {
          imports = [ ./nix/home.nix ];
          services.carrel.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.carrel;
        };

      # The module in a VM: the service starts and answers, and keeps strangers out.
      checks.x86_64-linux.module = nixpkgs.legacyPackages.x86_64-linux.testers.runNixOSTest {
        name = "carrel-module";
        nodes.machine = {
          imports = [ self.nixosModules.default ];
          services.carrel = {
            enable = true;
            listen = "0.0.0.0:8080";
            environmentFile = "/etc/carrel.env";
          };
          environment.etc."carrel.env".text = "CARREL_SERVE_TOKEN=test-token";
        };
        testScript = ''
          machine.wait_for_unit("carrel.service")
          machine.wait_until_succeeds("curl -sf http://127.0.0.1:8080/health")
          machine.fail("curl -sf http://127.0.0.1:8080/mcp")
          machine.succeed("curl -sf -H 'Authorization: Bearer test-token' -H 'Accept: application/json, text/event-stream' -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{},\"clientInfo\":{\"name\":\"t\",\"version\":\"1\"}}}' http://127.0.0.1:8080/mcp | grep carrel")
          machine.succeed("test -d /var/lib/carrel/library")
        '';
      };

      formatter = eachSystem (pkgs: pkgs.nixfmt);
    };
}
