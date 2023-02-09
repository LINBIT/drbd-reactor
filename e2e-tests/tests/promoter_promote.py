import time

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

    config = ReactorConfig(
            promoter=[Promoter(
                resources={
                    'res': PromoterResource(
                        dependencies_as='Requires',
                        start=[dummy_service.dummy_service_unit(device)]
                        )})])
    reactortest.deploy_reactor(config, 'drbd-res.toml', cluster.nodes)

    for _ in range(20):
        started_nodes = []
        for node in cluster.nodes:
            if reactortest.dummy_service_started(node, device):
                started_nodes.append(node)

        match started_nodes:
            case []:
                time.sleep(0.5)
            case [node]:
                reactortest.log(f'service started on {node}')
                break
            case _:
                raise AssertionError('service started on multiple nodes')
    else:
        raise AssertionError('service did not start on any node')
