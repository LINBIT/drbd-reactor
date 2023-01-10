"""
Configuration for a systemd service useful for testing DRBD Reactor.

The service template "dummy" defines an example service that writes to the
device given as the instance name.
"""

import string


dummy_service_path = '/etc/systemd/system/dummy@.service'

dummy_service_script_path = '/usr/local/bin/dummy_service.sh'

dummy_service_status_path = '/tmp/dummy-service.status'

dummy_service_template = f'''\
[Unit]
Description=Run dummy service on %I

[Service]
ExecStart=/bin/sh "{dummy_service_script_path}" %I
'''

dummy_service_script = f'''\
# Ensure that DRBD is writable by writing a fixed pattern.
cat /dev/zero | tr '\0' a | dd of="$1" bs=4K count=1 oflag=direct || exit 1

# Notify test suite.
echo "$1" >> {dummy_service_status_path}
'''


def dummy_service_unit(device: str) -> str:
    return f'dummy@{systemd_escape(device)}.service'


def systemd_escape(s: str) -> str:
    return ''.join([_escape_code_point(c) for c in s])


def _escape_code_point(c: str) -> str:
    match c:
        case '/':
            return '-'
        case x if c in ':_.' + string.ascii_letters + string.digits:
            return x
        case _:
            return ''.join([f'\\x{b:02x}' for b in c.encode('utf-8')])
