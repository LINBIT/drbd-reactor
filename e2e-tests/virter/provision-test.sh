#!/bin/sh

# This script mimics the test image build step performed by vmshed. This is
# useful for using the test image for local testing.

set -e

repository_packages="drbd-utils=${DRBD_UTILS_VERSION:-9.29.0}"

# Escape the comma so that it is part of the override value. An unescaped comma
# separates key-value pairs in the override.
repository_packages="${repository_packages}\\,drbd-reactor=${DRBD_REACTOR_VERSION:-1.0.0}"

virter image build --provision virter/provision-test.toml \
	--set "values.DrbdVersion=${DRBD_VERSION:-9.1.16}" \
	--set "values.RepositoryPackages=${repository_packages}" \
	--set 'values.RepositoryURL=https://nexus.at.linbit.com/repository/ci-yum/rhel9/' \
	--set 'values.ReleaseRepositoryURL=https://nexus.at.linbit.com/repository/packages-linbit-com/yum/rhel9.0/drbd-9/$basearch/' \
	alma-9-drbd-k70 alma-9-drbd-k70-r
