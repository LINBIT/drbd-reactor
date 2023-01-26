# DRBD Reactor end-to-end tests

This test suite tests DRBD Reactor.

## Getting started

This tutorial describes how to run a test with Virter but without vmshed. This
is convenient for development.

First install:

* `parallel`
* `jq`
* [`rq`](https://github.com/dflemstr/rq)
* [`virter`](https://github.com/LINBIT/virter)

Then run:

```
# Build base image
git clone https://github.com/LINBIT/drbd9-tests.git
make -C drbd9-tests/virter base_image_alma-9-drbd-k70

# Build test image
./virter/provision-test.sh

# Build test suite container image
make e2e_docker_image

# Start VMs
virter vm run --count 3 --name d --id 10 --disk name=data,size=2G,bus=scsi --wait-ssh alma-9-drbd-k70-r
parallel --tag virsh snapshot-create-as --name base {} ::: d-{10..12}

# Run test
virter vm exec -p virter/run.toml --set env.TEST_NAME=promoter_promote d-{10..12}

# Revert VMs so that they are ready to run the next test
parallel --tag virsh snapshot-revert --snapshotname base {} ::: d-{10..12}
```

## Node requirements

The following packages should be installed on the test nodes:

* `drbd-reactor`
* `drbd-utils`
* DRBD kernel module

An LVM volume group named `scratch` should exist.

## Test suite requirements

The test suite requires Python 3.10+. The following Python packages are required:

* `lbpytest`
* `toml`

## Running a test

To run a test, execute:

```
./drbd_reactor_test.py --test promoter_promote d-{10..12}
```

## Running the test suite with `vmshed`

The following is required to run the test suite with `vmshed`:

* A working installation of Virter.
* The `vmshed` binary.
* The DRBD base image. See [Getting started](#getting-started). There is no
  need to build the test image.
* The test suite container image. See [Getting started](#getting-started).
* Access to a package repository containing the DRBD, DRBD Utils and DRBD
  Reactor packages.

Once these requirements are met, `vmshed` can be run:

```
./virter/drbd-reactor-vmshed.sh
```
