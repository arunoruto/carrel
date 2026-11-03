{
  lib,
  callPackage,
  craneLib,
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
  # The dependencies alone, built again only when Cargo.lock changes; CI keeps them between runs.
  cargoArtifacts = craneLib.buildDepsOnly common;
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
