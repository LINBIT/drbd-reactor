#!/bin/bash

set -e

cd /
mkdir -p /tmp/src/drbd-reactor
cd /tmp/src/drbd-reactor && cp -r /src . && cd ./src
# allow "make rpm" building from a tarball (that does not have a .git)
# "make deb" will still fail from tarballs as they usually don't have a debian/ included
test -d .git ||
	{	git config --global user.email 'invalid@linbit.com' ;\
		git config --global user.name "Container User" \;
		git init ;\
		git add -A ;\
		git commit -am 'initial' ; }

source "$HOME/.cargo/env" || true
# always create a (dirty) release tarball and build it as usual
# we need it as source for rpm builds and it also tests the release bits
VERSION="$(awk -F '=' '/^version/ {gsub(/"/, "", $2); gsub(/ /, "", $2); print $2}' Cargo.toml)"
install /dev/null /usr/bin/lbvers.py
make debrelease VERSION="$VERSION"
mkdir /tmp/build && mv "./drbd-reactor-${VERSION}.tar.gz" /tmp/build
cd /tmp/build && tar -xvf "./drbd-reactor-${VERSION}.tar.gz" && cd "./drbd-reactor-${VERSION}"

case $1 in
	rpm)
		mkdir -p "$(rpm -E "%_topdir")/SOURCES"
		mv "../drbd-reactor-${VERSION}.tar.gz" "$(rpm -E "%_topdir")/SOURCES"
		rpmbuild -bb drbd-reactor.spec
		find ~/rpmbuild/RPMS/ -name "*.rpm" -exec cp {} /out \;
		;;
	deb)
		debuild -us -uc -i -b
		find /tmp/build -name "*.deb" -exec cp {} /out \;
		;;
esac
