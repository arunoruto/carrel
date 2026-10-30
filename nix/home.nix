# Home Manager: carrel as user services.
# - services.carrel.consume: `carrel consume` whenever a file lands in the watched folders, and hourly in
#   case one slipped by. Without a terminal it adds nothing unless `[consume] auto = true`, and tells you
#   through the config's [notify] what it found or added.
# - services.carrel.web: `carrel web`, the library in your browser at http://127.0.0.1:7700.
# - services.carrel.docling.package: extract text with Docling, in the shell and in these services.
{
  config,
  lib,
  ...
}:

let
  cfg = config.services.carrel;
  carrel = lib.getExe cfg.package;
  docling = lib.getExe cfg.docling.package;
  environment = lib.optional (cfg.docling.package != null) "CARREL_DOCLING=${docling}";
in
{
  options.services.carrel = {
    package = lib.mkOption {
      type = lib.types.package;
      description = "The carrel package.";
    };

    docling.package = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = null;
      example = lib.literalExpression "pkgs.docling";
      description = ''
        Docling, to extract text with better tables and reading order than PyMuPDF, but slower; it brings
        PyTorch. It is installed and carrel uses it. Null: carrel uses `docling` if it is on PATH, else PyMuPDF.
      '';
    };

    consume = {
      enable = lib.mkEnableOption "carrel consume on new PDFs in your Downloads (Linux, systemd)";

      folders = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ "${config.home.homeDirectory}/Downloads" ];
        defaultText = lib.literalExpression ''[ "''${config.home.homeDirectory}/Downloads" ]'';
        description = "The folders to watch. They replace `[consume] folders` from the config.";
      };
    };

    web = {
      enable = lib.mkEnableOption "carrel web, the library in your browser (Linux, systemd)";

      listen = lib.mkOption {
        type = lib.types.str;
        default = "127.0.0.1:7700";
        description = "Where to serve it; loopback only. `carrel web --get-url` prints the link to open.";
      };
    };
  };

  config = lib.mkMerge [
    { home.packages = lib.mkIf (cfg.consume.enable || cfg.web.enable) [ cfg.package ]; }

    (lib.mkIf (cfg.docling.package != null) {
      home.packages = [ cfg.docling.package ];
      home.sessionVariables.CARREL_DOCLING = docling;
    })

    (lib.mkIf cfg.consume.enable {
      systemd.user.services.carrel-consume = {
        Unit.Description = "carrel: pick up new papers";
        Service = {
          Type = "oneshot";
          Environment = environment;
          ExecStart = "${carrel} consume ${lib.escapeShellArgs cfg.consume.folders}";
        };
      };

      # A run still going misses changes meanwhile; the timer catches those.
      systemd.user.paths.carrel-consume = {
        Unit.Description = "carrel: watch for new papers";
        Path.PathChanged = cfg.consume.folders;
        Install.WantedBy = [ "default.target" ];
      };

      systemd.user.timers.carrel-consume = {
        Unit.Description = "carrel: look for new papers hourly";
        Timer = {
          OnCalendar = "hourly";
          Persistent = true;
        };
        Install.WantedBy = [ "timers.target" ];
      };
    })

    (lib.mkIf cfg.web.enable {
      systemd.user.services.carrel-web = {
        Unit.Description = "carrel: the library in your browser";
        Service = {
          Environment = environment;
          ExecStart = "${carrel} web --no-open --listen ${cfg.web.listen}";
          Restart = "on-failure";
        };
        Install.WantedBy = [ "default.target" ];
      };
    })
  ];
}
