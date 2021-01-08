#!/bin/bash

SCRIPTPATH="$( cd "$(dirname "$0")" >/dev/null 2>&1 ; pwd -P )"
TMDIR="$(mktemp -d)"

cd /
cd $TMPDIR && cp -r /src/* .

source "$HOME/.cargo/env"
case $1 in
	rpm)
		USER=builder cargo rpm init
		sed -i 's/^Release:.*/Release: @@RELEASE@@/' .rpm/drbdd.spec
		cargo rpm build
		find ./target/release/rpmbuild/RPMS/ -name "*.rpm" -exec cp {} /out \;
		;;
	deb)
		cargo deb
		find ./target/debian/ -name "*.deb" -exec cp {} /out \;
		;;
esac
