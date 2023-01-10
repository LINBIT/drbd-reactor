from collections.abc import MutableMapping, MutableSequence
from dataclasses import dataclass, field
from typing import cast

import toml


@dataclass
class ReactorPromoterResource(object):
    dependencies_as: str | None = None
    target_as: str | None = None
    start: MutableSequence[str] = field(default_factory=list)


@dataclass
class ReactorPromoter(object):
    resources: MutableMapping[str, ReactorPromoterResource] = field(default_factory=dict)


@dataclass
class ReactorConfig(object):
    promoter: MutableSequence[ReactorPromoter] = field(default_factory=list)


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
