from common import drbd_config
from common import dummy_service
from common import reactortest
from common.reactor_config import ReactorConfig, Promoter, PromoterResource


def test(cluster: reactortest.Cluster) -> None:
    assert len(cluster.nodes) == 3

    res = reactortest.define_drbd_resource(cluster.nodes, 'res')
    reactortest.add_drbd_volume(res)
    reactortest.deploy_drbd(res, cluster.nodes)

    device = drbd_config.drbd_device(res.volumes[0].minor_number)

    preferred_node = cluster.nodes[-1]

    config = ReactorConfig(
            promoter=[Promoter(
                resources={
                    'res': PromoterResource(
                        dependencies_as='Requires',
                        start=[dummy_service.dummy_service_unit(device)],
                        preferred_nodes=[preferred_node.hostname]
                        )})])
    reactortest.deploy_reactor(config, 'drbd-res.toml', cluster.nodes)

    reactortest.poll_nodes(nodes=cluster.nodes,
            condition=lambda node: reactortest.dummy_service_started(node, device),
            description='service started',
            expected_node=preferred_node)
