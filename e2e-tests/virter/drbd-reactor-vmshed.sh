#!/bin/sh

set -e

repository_packages="drbd-utils=${DRBD_UTILS_VERSION:-9.28.0}"

# Escape the comma so that it is part of the override value. An unescaped comma
# separates key-value pairs in the override.
repository_packages="${repository_packages}\\,drbd-reactor=${DRBD_REACTOR_VERSION:-1.0.0}"

vmshed \
	--nvms "${LINBIT_CI_MAX_CPUS:-$(nproc)}" \
	--vms virter/vms.toml \
	--tests virter/tests.toml \
	--set "values.DrbdVersion=${DRBD_VERSION:-9.1.16}" \
	--set "values.RepositoryPackages=${repository_packages}" \
	${REACTOR_TEST_IMAGE:+--set "values.TestSuiteImage=${REACTOR_TEST_IMAGE}"} \
	"$@"
