{
  description = "ffspot";
  inputs = {
    nixpkgs.url = "nixpkgs/23.11";
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
  };
  outputs = { self, nixpkgs, flake-utils, naersk }:
    flake-utils.lib.eachDefaultSystem (system:
      let pkgs = nixpkgs.legacyPackages.${system}; in {
        devShell = import ./shell.nix { inherit pkgs; };
        defaultPackage = (naersk.lib.${system}.override {
          cargo = pkgs.cargo;
          rustc = pkgs.rustc;
        }).buildPackage {
          src = ./.;
          nativeBuildInputs = with pkgs; [
            rustc
            cargo
          ];
          buildInputs = with pkgs; [
            ffmpeg
          ];
        };
        formatter = pkgs.nixpkgs-fmt;
      }
    );
}

