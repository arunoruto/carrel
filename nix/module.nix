# services.carrel: `carrel serve` as a systemd service, the library in /var/lib/carrel.
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.carrel;
  toml = pkgs.formats.toml { };
  # carrel reads $XDG_CONFIG_HOME/carrel/config.toml.
  configDir = pkgs.linkFarm "carrel-config" [
    {
      name = "carrel/config.toml";
      path = toml.generate "config.toml" cfg.settings;
    }
  ];
in
{
  options.services.carrel = {
    enable = lib.mkEnableOption "carrel serve, a paper library shared by a group and its AIs";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The carrel package.";
    };

    listen = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1:8080";
      example = "0.0.0.0:8080";
      description = "Where to serve MCP over HTTP. Anything but loopback needs a token (`CARREL_SERVE_TOKEN`).";
    };

    settings = lib.mkOption {
      inherit (toml) type;
      default = { };
      example = lib.literalExpression ''
        {
          remote = {
            type = "s3";
            endpoint = "https://s3.example.org";
            region = "garage";
            bucket = "papers";
            access_key = "GK...";
          };
          serve.every = 10;
        }
      '';
      description = ''
        carrel's config.toml, without the library (it's /var/lib/carrel/library). Keep secrets out of
        it: they would land in the Nix store. Put them in `environmentFile` instead.
      '';
    };

    environmentFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = "/run/secrets/carrel.env";
      description = ''
        Secrets as environment variables: `CARREL_SERVE_TOKEN`, `AWS_SECRET_ACCESS_KEY`, `ADS_API_TOKEN`,
        `S2_API_KEY`, `OPENALEX_API_KEY`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.carrel = {
      description = "carrel paper library";
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      after = [ "network-online.target" ];
      environment = {
        CARREL_HOME = "/var/lib/carrel/library";
        XDG_CONFIG_HOME = "${configDir}";
        XDG_CACHE_HOME = "/var/cache/carrel";
      };
      serviceConfig = {
        ExecStart = "${lib.getExe cfg.package} serve --listen ${cfg.listen}";
        EnvironmentFile = lib.mkIf (cfg.environmentFile != null) cfg.environmentFile;
        Restart = "on-failure";
        DynamicUser = true;
        StateDirectory = "carrel";
        CacheDirectory = "carrel";
        NoNewPrivileges = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
      };
    };
  };
}
