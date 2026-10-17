{
  lib,
  rustPlatform,
  python3,
}:

let
  cargoToml = (lib.importTOML ../Cargo.toml).package;
  # The extractor's Python, from nixpkgs instead of downloaded by uv at runtime: works offline.
  python = python3.withPackages (ps: [ ps.pymupdf4llm ]);
in
rustPlatform.buildRustPackage {
  pname = cargoToml.name;
  inherit (cargoToml) version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
      ../tests
    ];
  };
  cargoLock.lockFile = ../Cargo.lock;

  # Baked in as the default; the config's `extractor` and CARREL_EXTRACTOR still override it.
  env.CARREL_DEFAULT_EXTRACTOR = ''${python}/bin/python -I -c "$CARREL_EXTRACT_PY" "$PDF"'';

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
