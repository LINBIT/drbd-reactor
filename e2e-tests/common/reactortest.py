from collections.abc import Iterable, Mapping, Sequence
from io import StringIO
import pipes
import socket
from subprocess import CalledProcessError
import sys
from typing import cast, TextIO

from lbpytest.controlmaster import SSH

from .drbd_config import DRBDVolume, drbd_config, DRBDNode, DRBDResource
from . import dummy_service
from .reactor_config import dump_reactor_config, ReactorConfig


lvm_volume_group = 'scratch'


def log(text: str) -> None:
    print(text, file=sys.stderr)
    sys.stderr.flush()


class Cluster(object):
    def __init__(self, hostnames) -> None:
        self.nodes = [Node(name) for name in hostnames]


class Node(object):
    def __init__(self, name) -> None:
        self.name = name
        self.ssh = SSH(self.name, timeout=30)
        self.addr = socket.gethostbyname(name)

        install_dummy_service(self)

    def __repr__(self) -> str:
        return self.name

    def run(self, cmd: Iterable[str], *,
            quote: bool = True,
            catch: bool = False,
            return_stdout: bool = False,
            stdin: TextIO | bool = False,
            stdout: TextIO = sys.stderr,
            stderr: TextIO = sys.stderr,
            env: Mapping[str, str] = {},
            timeout: int | None = None) -> str | None:
        """
        Run a command via SSH on the target node.

        :param cmd: the command
        :param quote: use shell quoting to prevent environment variable substitution in commands
        :param catch: report command failures on stderr rather than raising an exception
        :param return_stdout: return the stdout returned by the command instead of printing it
        :param stdin: standard input to command
        :param stdout: standard output from command
        :param stderr: standard error from command
        :param env: extra environment variables which will be exported to the command
        :param timeout: command timeout in seconds
        :returns: nothing, or a string if return_stdout is True
        :raise CalledProcessError: when the command fails (unless catch is True)
        """

        stdout_capture: StringIO | None = None

        if return_stdout:
            stdout_capture = StringIO()
            stdout = stdout_capture

        if quote:
            cmd_string = ' '.join(pipes.quote(str(x)) for x in cmd)
        else:
            cmd_string = ' '.join(cmd)

        log(f'{self.name}: {cmd_string}')
        result = self.ssh.run(cmd_string, env=env, stdin=stdin, stdout=stdout, stderr=stderr, timeout=timeout)
        if result != 0:
            if catch:
                log(f'error: \'{cmd_string}\' failed ({result})')
            else:
                raise CalledProcessError(result, cmd_string)

        if stdout_capture:
            return stdout_capture.getvalue().strip()

    def write_file(self, path: str, content: str) -> None:
        self.run(['sh', '-c', f'cat > "{path}"'], stdin=StringIO(content))


def volume_name(resource_name: str, volume_number: int) -> str:
    return f'{resource_name}_{volume_number:03}'


def volume_path(resource_name: str, volume_number: int) -> str:
    return f'/dev/{lvm_volume_group}/{volume_name(resource_name, volume_number)}'


def create_storage_volume(node, volume: DRBDVolume) -> None:
    node.run(['lvcreate', '--wipesignatures', 'y', '--yes',
        '--name', volume.storage_name,
        '--size', volume.size,
        lvm_volume_group])


def drbd_config_file_path(resource_name: str) -> str:
    return f'/etc/drbd.d/{resource_name}.res'


def define_drbd_resource(nodes: Iterable[Node], resource_name: str, port: int = 8400) -> DRBDResource:
    return DRBDResource(
            name=resource_name,
            nodes=[DRBDNode(
                name=node.name,
                addr=node.addr) for node in nodes],
            port=port,
            # Apply the recommended options by default.
            options={
                 'auto-promote': 'no',
                 'quorum': 'majority',
                 'on-no-quorum': 'io-error',
                 'on-no-data-accessible': 'io-error',
                 'on-suspended-primary-outdated': 'force-secondary'})


def add_drbd_volume(resource: DRBDResource, size: str = '20M', minor_number: int = 0, volume_number: int = 0):
    resource.volumes.append(DRBDVolume(
        volume_number=volume_number,
        minor_number=minor_number,
        size=size,
        storage_name=volume_name(resource.name, volume_number),
        path=volume_path(resource.name, volume_number)))


def deploy_drbd(resource: DRBDResource, nodes: Sequence[Node]) -> None:
    config_str = drbd_config(resource)

    for node in nodes:
        for volume in resource.volumes:
            create_storage_volume(node, volume)
        node.write_file(drbd_config_file_path(resource.name), config_str)
        node.run(['drbdadm', 'create-md', '--force', resource.name])
        node.run(['drbdadm', 'adjust', resource.name])

    for volume in resource.volumes:
        nodes[0].run(['drbdadm', 'new-current-uuid', '--clear-bitmap', f'{resource.name}/{volume.volume_number}'])


def reactor_config_file_path(filename: str) -> str:
    return f'/etc/drbd-reactor.d/{filename}'


def restart_reactor(node: Node) -> None:
    node.run(['systemctl', 'restart', 'drbd-reactor'])


def deploy_reactor(config: ReactorConfig, filename: str, nodes: Iterable[Node]) -> None:
    config_str = dump_reactor_config(config)
    for node in nodes:
        node.write_file(reactor_config_file_path(filename), config_str)
        restart_reactor(node)


def install_dummy_service(node: Node) -> None:
    node.write_file(dummy_service.dummy_service_status_path, '')
    node.write_file(dummy_service.dummy_service_script_path, dummy_service.dummy_service_script)
    node.write_file(dummy_service.dummy_service_path, dummy_service.dummy_service_template)


def dummy_service_started(node: Node, device: str) -> bool:
    status = node.run(['cat', dummy_service.dummy_service_status_path],
            return_stdout=True)
    return device in cast(str, status).splitlines()
