# services.carrel.consume (Home Manager): `carrel consume` whenever a file lands in the watched folders,
# and hourly in case one slipped by. Without a terminal it adds nothing unless `[consume] auto = true`,
# and tells you through the config's [notify] what it found or added.
{
  config,
  lib,
  ...
}:

let
  cfg = config.services.carrel.consume;
in
{
  options.services.carrel.consume = {
    enable = lib.mkEnableOption "carrel consume on new PDFs in your Downloads (Linux, systemd)";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The carrel package.";
    };

    folders = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "${config.home.homeDirectory}/Downloads" ];
      defaultText = lib.literalExpression ''[ "''${config.home.homeDirectory}/Downloads" ]'';
      description = "The folders to watch. They replace `[consume] folders` from the config.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.user.services.carrel-consume = {
      Unit.Description = "carrel: pick up new papers";
      Service = {
        Type = "oneshot";
        ExecStart = "${lib.getExe cfg.package} consume ${lib.escapeShellArgs cfg.folders}";
      };
    };

    # A run still going misses changes meanwhile; the timer catches those.
    systemd.user.paths.carrel-consume = {
      Unit.Description = "carrel: watch for new papers";
      Path.PathChanged = cfg.folders;
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
  };
}
