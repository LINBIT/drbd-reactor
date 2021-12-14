#!/bin/bash

# [[umh]]
# [[umh.device]]
# resource-name = "foo"
# old.quorum = true
# new.quorum = false
# command = "/usr/local/bin/on-no-quorum-io-error.sh"
# name = "quorum lost"
# [umh.device.env]
# TIMEOUT = "30"
#
# [[umh.device]]
# resource-name = "foo"
# old.quorum = false
# new.quorum = true
# command = "/usr/local/bin/on-no-quorum-io-error.sh"
# name = "quorum gained"

: "${TIMEOUT:=60}"

die() {
	>&2 echo "$1"
	exit 1
}

set_io_error() {
	drbdsetup resource-options --on-no-quorum io-error --on-no-data io-error "${DRBD_RES_NAME}"
}

set_suspend_io() {
	drbdsetup resource-options --on-no-quorum suspend-io --on-no-data suspend-io "${DRBD_RES_NAME}"
}

is_in_use() {
	role=$(drbdadm role "${DRBD_RES_NAME}")

	[[ $role == Primary ]]
}

has_quorum() {
	drbdsetup events2 --now "${DRBD_RES_NAME}" | grep -q "quorum:yes"
}

case "${DRBD_NEW_QUORUM}" in
	true)
		echo "Got quorum"
		set_suspend_io
		;;
	false)
		echo "Lost quorum"
		echo "Checking if in use"
		is_in_use || { set_io_error; exit 0; }
		echo "Sleeping ${TIMEOUT}..."
		sleep "${TIMEOUT}"
		echo "Checking for quorum"
		has_quorum && exit 0
		echo "Setting io-error"
		set_io_error
		;;
	*) die "Quorum state '${DRBD_NEW_QUORUM}' can not happen" ;;
esac

