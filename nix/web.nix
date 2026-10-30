# The web UI's built files (web/dist), which the carrel package embeds.
{
  lib,
  buildNpmPackage,
}:

buildNpmPackage {
  pname = "carrel-web";
  version = (lib.importTOML ../Cargo.toml).package.version;

  src = lib.fileset.toSource {
    root = ../web;
    fileset = lib.fileset.difference ../web (
      lib.fileset.unions [
        (lib.fileset.maybeMissing ../web/node_modules)
        (lib.fileset.maybeMissing ../web/dist)
      ]
    );
  };
  # Update after changing web/package-lock.json: `nix run nixpkgs#prefetch-npm-deps web/package-lock.json`.
  npmDepsHash = "sha256-q9nOyfIrHcOJUnJgfsMXMDRJj4xZfQLN4FudZ37HWUI=";

  installPhase = ''
    runHook preInstall
    cp -r dist $out
    runHook postInstall
  '';
}
