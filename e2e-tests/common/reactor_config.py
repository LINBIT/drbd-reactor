from collections.abc import MutableMapping, MutableSequence
from dataclasses import dataclass, field
from typing import cast

import toml


@dataclass
class PromoterResource(object):
    dependencies_as: str | None = None
    target_as: str | None = None
    start: MutableSequence[str] = field(default_factory=list)
    preferred_nodes: str | None = None


@dataclass
class Promoter(object):
    resources: MutableMapping[str, PromoterResource] = field(default_factory=dict)


@dataclass
class WithOperator(object):
    operator: str
    value: str


@dataclass
class UMHResourceUpdateState(object):
    role: str | WithOperator


@dataclass
class UMHResource(object):
    command: str
    event_type: str
    resource_name: str
    old: UMHResourceUpdateState
    new: UMHResourceUpdateState


@dataclass
class UMH(object):
    resource: MutableSequence[UMHResource] = field(default_factory=list)


@dataclass
class ReactorConfig(object):
    promoter: MutableSequence[Promoter] = field(default_factory=list)
    umh: MutableSequence[UMH] = field(default_factory=list)


def _rename_key(k: str) -> str:
    """
    Class members use Python style names internally. For serialization to toml
    we need to rename them.
    """
    return k.replace("_", "-")


def to_plain(item):
    match item:
        case dict():
            return {_rename_key(k): to_plain(v) for k, v in item.items()}
        case list() | tuple():
            return [to_plain(x) for x in item]
        case object(__dict__=d):
            return to_plain(d)
        case _:
            return item


def dump_reactor_config(config: ReactorConfig):
    d = to_plain(config)
    return toml.dumps(cast(dict, d))
