#!/bin/bash

SCRIPTPATH="$( cd "$(dirname "$0")" >/dev/null 2>&1 ; pwd -P )"

cd /
mkdir -p /tmp/src/drbdd
cd /tmp/src/drbdd && cp -r /src . && cd ./src

source "$HOME/.cargo/env"
case $1 in
	rpm)
		VERSION="$(awk -F '=' '/^version/ {gsub(/"/, "", $2); gsub(/ /, "", $2); print $2}' Cargo.toml)"
		install /dev/null /usr/bin/lbvers.py
		make debrelease VERSION="$VERSION"
		mkdir -p "$(rpm -E "%_topdir")/SOURCES"
		mv "./drbdd-${VERSION}.tar.gz" "$(rpm -E "%_topdir")/SOURCES"
		rpmbuild -bb drbdd.spec
		find ~/rpmbuild/RPMS/ -name "*.rpm" -exec cp {} /out \;
		;;
	deb)
		debuild -us -uc -i -b
		find /tmp/src -name "*.deb" -exec cp {} /out \;
		;;
esac
