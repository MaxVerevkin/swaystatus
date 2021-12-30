#!/bin/sh
# Use this script when installing via `cargo` in order to be able to use the default icons/themes.
# If installed via a package manager you do not need to run this script.
mkdir -p ~/.local/share/swaystatus
cp -r files/* ~/.local/share/swaystatus/