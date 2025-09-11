{ pkgs ? import <nixpkgs> {} }:
  pkgs.mkShell {
    # build time deps
    nativeBuildInputs = with pkgs; [ rustc cargo ];
    # other useful pkgs
    # for debian/changelog generation use internal build-helpers:mini-gbp-dch.sh
    # git for release commit, make for make debrelease, debian-devscripts for dch
    # python + toml for build-helpers:lbvers.py
    packages = with pkgs; [ git gnumake python3 python3Packages.toml debian-devscripts osv-scanner clippy ];
}
