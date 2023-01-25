# DRBD Reactor end-to-end tests

This test suite tests DRBD Reactor.

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
