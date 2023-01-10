from collections.abc import MutableMapping, MutableSequence, Sequence
from dataclasses import dataclass, field
from typing import Protocol


class StringSink(Protocol):
    def append(self, __object: str) -> None:
        ...


class ConfigBlock(object):
    INDENT = '    '

    def __init__(self, output: StringSink, name: str) -> None:
        self.output = output
        self.name = name

    def __enter__(self):
        self.write_no_indent(f'{self.name} {{\n')
        return self

    def __exit__(self, *ignore_exception) -> None:
        self.write_no_indent('}\n')

    def write_no_indent(self, content: str) -> None:
        return self.output.append(content)

    def write(self, text: str) -> None:
        content = f'{self.INDENT}{text}'
        if not content.endswith('\n'):
            content += '\n'

        self.write_no_indent(content)

    def append(self, line: str) -> None:
        self.output.append(f'{self.INDENT}{line}')


@dataclass
class DRBDNode(object):
    name: str
    addr: str


@dataclass
class DRBDVolume(object):
    volume_number: int
    minor_number: int
    size: str
    storage_name: str
    path: str


@dataclass
class DRBDResource(object):
    name: str
    nodes: Sequence[DRBDNode]
    port: int
    options: MutableMapping[str, str] = field(default_factory=dict)
    volumes: MutableSequence[DRBDVolume] = field(default_factory=list)


def drbd_config(resource: DRBDResource) -> str:
    text = []

    with ConfigBlock(text, f'resource "{resource.name}"') as resource_block:
        with ConfigBlock(resource_block, 'options') as options_block:
            for key, value in resource.options.items():
                options_block.write(f'{key} {value};')

        for node_id, n in enumerate(resource.nodes):
            _config_host(resource_block, resource, node_id, n)

        for start, n1 in enumerate(resource.nodes):
            for n2 in resource.nodes[start + 1:]:
                with ConfigBlock(resource_block, 'connection') as connection_block:
                    _config_one_host_addr(connection_block, resource, n1)
                    _config_one_host_addr(connection_block, resource, n2)

    return "".join(text)


def _config_host(block: ConfigBlock, resource: DRBDResource, node_id: int, node: DRBDNode) -> None:
    with ConfigBlock(block, f'on {node.name}') as node_block:
        node_block.write(f'node-id {node_id};')

        for volume in resource.volumes:
            with ConfigBlock(node_block, f'volume {volume.volume_number}') as volume_block:
                device = drbd_device(volume.minor_number)
                volume_block.write(f'device {device};')
                volume_block.write(f'disk {volume.path};')
                volume_block.write('meta-disk internal;')


def _config_one_host_addr(block: ConfigBlock, resource: DRBDResource, node: DRBDNode) -> None:
    block.write(f'host {node.name} address {node.addr}:{resource.port};')


def drbd_device(minor_number: int):
    return f'/dev/drbd{minor_number}'
