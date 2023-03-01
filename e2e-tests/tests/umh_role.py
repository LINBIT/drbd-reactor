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

    reactortest.poll_nodes(nodes=cluster.nodes,
            condition=lambda node: reactortest.dummy_service_started(node, device),
            description='command ran',
            expected_node=primary_node)
