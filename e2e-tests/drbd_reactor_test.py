#!/usr/bin/env python3

import argparse
import importlib

from common import reactortest


def main() -> None:
    parser = argparse.ArgumentParser(
                    prog='drbd_reactor_test.py',
                    description='Run a DRBD Reactor end-to-end test.')

    parser.add_argument('host', nargs='*')
    parser.add_argument('-t', '--test', help='the name of the test to run', required=True)

    args = parser.parse_args()

    mod = importlib.import_module(f'tests.{args.test}')

    cluster = reactortest.Cluster(args.host)
    mod.test(cluster)


if __name__ == "__main__":
    main()
