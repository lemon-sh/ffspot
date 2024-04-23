{ pkgs ? import <nixpkgs> { } }:

pkgs.mkShell {
  name = "ffspot";
  nativeBuildInputs = with pkgs; [
    rustc
    cargo
  ];
  buildInputs = with pkgs; [
    ffmpeg
  ];
}
