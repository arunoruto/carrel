{
  lib,
  callPackage,
  craneLib,
  linkFarm,
  python3,
}:

let
  cargoToml = (lib.importTOML ../Cargo.toml).package;
  # The extractor's Python, from nixpkgs instead of downloaded by uv at runtime: works offline.
  python = python3.withPackages (ps: [ ps.pymupdf4llm ]);
  web = callPackage ./web.nix { };
  common = {
    pname = cargoToml.name;
    inherit (cargoToml) version;
    src = lib.fileset.toSource {
      root = ../.;
      fileset = lib.fileset.unions [
        ../Cargo.toml
        ../Cargo.lock
        ../build.rs
        ../src
        ../tests
      ];
    };
    strictDeps = true;
  };
  # The dependencies alone, from a stub of carrel with its own version left out: built again only when the
  # dependencies change, not with each release. Your Nix store and CI keep them.
  unversioned =
    file: from: to:
    builtins.toFile (baseNameOf file) (
      builtins.replaceStrings [ from ] [ to ] (builtins.readFile file)
    );
  cargoLock =
    unversioned ../Cargo.lock
      ''
        name = "carrel"
        version = "${cargoToml.version}"
      ''
      ''
        name = "carrel"
        version = "0.0.0"
      '';
  stub = linkFarm "carrel-deps-src" [
    {
      name = "Cargo.toml";
      path =
        unversioned ../Cargo.toml
          ''
            version = "${cargoToml.version}"
          ''
          ''
            version = "0.0.0"
          '';
    }
    {
      name = "Cargo.lock";
      path = cargoLock;
    }
    {
      name = "build.rs";
      path = builtins.toFile "build.rs" "fn main() {}";
    }
    {
      name = "src/main.rs";
      path = builtins.toFile "main.rs" "fn main() {}";
    }
    {
      name = "src/lib.rs";
      path = builtins.toFile "lib.rs" "";
    }
  ];
  cargoArtifacts = craneLib.buildDepsOnly (
    removeAttrs common [ "src" ]
    // {
      # Given outright, so nothing is read from a build's output while evaluating.
      dummySrc = stub;
      cargoVendorDir = craneLib.vendorCargoDeps { inherit cargoLock; };
      version = "0.0.0";
    }
  );
in
craneLib.buildPackage (
  common
  // {
    inherit cargoArtifacts;

    # PyMuPDF's command, always available; `auto` still prefers Docling (not included: it brings PyTorch)
    # when it is on PATH, and the config's `extractor` and CARREL_EXTRACTOR override it.
    env.CARREL_DEFAULT_EXTRACTOR = ''${python}/bin/python -I -c "$CARREL_EXTRACT_PY" "$PDF"'';
    # `carrel web`'s UI, built into the binary.
    env.CARREL_WEB_DIST = web;

    passthru = { inherit web cargoArtifacts; };

    # `cargo test` runs the offline tests; the ones needing internet are #[ignore]d.

    meta = {
      inherit (cargoToml) description homepage;
      changelog = "${cargoToml.repository}/blob/main/CHANGELOG.md";
      license = with lib.licenses; [
        mit
        asl20
      ];
      mainProgram = "carrel";
      platforms = lib.platforms.unix;
    };
  }
)
