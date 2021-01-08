# umh

The user mode helper is a plugin that allows users to specify actions executed on arbitrary DRBD state
changes.

For example it will be possible for the user to run `somescript.sh` if the resource "foo" changes its disk
state from any state to `UpToDate`.

This plugin will be implemented soon, but the format of the rules is currently up for discussion among
developers.
