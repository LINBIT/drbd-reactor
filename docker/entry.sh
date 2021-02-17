#!/bin/bash

SCRIPTPATH="$( cd "$(dirname "$0")" >/dev/null 2>&1 ; pwd -P )"

cd /
mkdir -p /tmp/src/drbdd
cd /tmp/src/drbdd && cp -r /src/* .

source "$HOME/.cargo/env"
case $1 in
	rpm)
		USER=builder cargo rpm init
		(cd .rpm && patch < ../docker/drbdd.spec.patch)
		cargo rpm build
		find ./target/release/rpmbuild/RPMS/ -name "*.rpm" -exec cp {} /out \;
		;;
	deb)
		debuild -us -uc -i -b
		find /tmp/src -name "*.deb" -exec cp {} /out \;
		;;
esac
