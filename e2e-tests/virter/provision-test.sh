#!/bin/sh

# This script mimics the test image build step performed by vmshed. This is
# useful for using the test image for local testing.

set -e

repository_packages="drbd-utils=${DRBD_UTILS_VERSION:-9.35.0}"

# Escape the comma so that it is part of the override value. An unescaped comma
# separates key-value pairs in the override.
repository_packages="${repository_packages}\\,drbd-reactor=${DRBD_REACTOR_VERSION:-1.0.0}"

virter image build --provision virter/provision-test.toml \
	--set "values.DrbdVersion=${DRBD_VERSION:-9.3.4}" \
	--set "values.RepositoryPackages=${repository_packages}" \
	--set "values.RepositoryURL=${LINBIT_PULP_BASE_URL:-https://pulp.at.linbit.com}/pulp/content/rpm/rhel9/" \
	--set 'values.ReleaseRepositoryURL=http://pkg-linbit.at.linbit.com/yum/rhel9.0/drbd-9/$basearch/' \
	rhel-9-drbd-k70 rhel-9-drbd-k70-r
