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
./drbd_reactor_test.py --test promoter_promote d-{150..152}.test
```
