import time

from common import drbd_config
from common import dummy_service
from common import reactortest
from common.reactor_config import ReactorConfig, UMH, UMHResource, UMHResourceUpdateState, WithOperator


def test(cluster: reactortest.Cluster) -> None:
    assert len(cluster.nodes) == 3

    res = reactortest.define_drbd_resource(cluster.nodes, 'res')
    reactortest.add_drbd_volume(res)
    reactortest.deploy_drbd(res, cluster.nodes)

    device = drbd_config.drbd_device(res.volumes[0].minor_number)

    config = ReactorConfig(
            umh=[UMH(
                resource=[UMHResource(
                        command=f'/bin/sh {dummy_service.dummy_service_script_path} {device}',
                        event_type='Change',
                        resource_name='res',
                        old=UMHResourceUpdateState(
                            role=WithOperator(operator='NotEquals', value='Primary')),
                        new=UMHResourceUpdateState(role='Primary')
                        )])])
    reactortest.deploy_reactor(config, 'umh.toml', cluster.nodes)

    primary_node = cluster.nodes[0]
    primary_node.run(['drbdadm', 'wait-connect', 'res'])
    primary_node.run(['drbdadm', 'primary', 'res'])

    for _ in range(20):
        command_ran_nodes = []
        for node in cluster.nodes:
            if reactortest.dummy_service_started(node, device):
                command_ran_nodes.append(node)

        match command_ran_nodes:
            case []:
                time.sleep(0.5)
            case [node]:
                if node == primary_node:
                    reactortest.log(f'command ran on {primary_node}')
                    break
                else:
                    raise AssertionError(f'command ran on unexpected node {node}')
            case _:
                raise AssertionError('command ran on multiple nodes')
    else:
        raise AssertionError('command did not run on any node')
