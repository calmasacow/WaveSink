{
  description = "WaveSink release AppImage for NixOS";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = { nixpkgs, ... }:
    let
      pkgs = nixpkgs.legacyPackages.x86_64-linux;
      version = "0.9.4";
    in {
      packages.x86_64-linux.default = pkgs.appimageTools.wrapType2 {
        pname = "wavesink";
        inherit version;
        src = pkgs.fetchurl {
          url = "https://github.com/calmasacow/WaveSink/releases/download/v${version}/wavesink_${version}_amd64.AppImage";
          hash = "sha256-h3rRNa3qR0fBbA2xT9uIhGkayZmm1Jf1IDCnZQQzbyM=";
        };
        extraPkgs = pkgs: [ pkgs.pipewire ];
      };
    };
}
