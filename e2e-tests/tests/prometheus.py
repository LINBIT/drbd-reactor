import re

from common import reactortest
from common.reactor_config import ReactorConfig, Prometheus


prometheus_address = "0.0.0.0:9952"

# match 'drbdreactor_up 1' exactly from prometheus output
prometheus_pattern = re.compile(r'drbdreactor_up\s1')


def verify_prometheus_endpoint(node: reactortest.Node) -> bool:
    prometheus_output = reactortest.prometheus_endpoint_scrape(node, prometheus_address)
    if not prometheus_output:
        return False

    m = prometheus_pattern.search(prometheus_output)
    return bool(m)


def test(cluster: reactortest.Cluster) -> None:
    assert len(cluster.nodes) == 1

    config = ReactorConfig(
            prometheus=[Prometheus(
                enums=True,
                address=prometheus_address
                )])
    reactortest.deploy_reactor(config, 'prometheus.toml', cluster.nodes)

    reactortest.poll_nodes(nodes=cluster.nodes,
            condition=verify_prometheus_endpoint,
            description='match found')
