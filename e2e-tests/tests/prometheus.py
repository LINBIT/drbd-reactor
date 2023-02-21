import re
import time

from common import reactortest
from common.reactor_config import ReactorConfig, Prometheus


def test(cluster: reactortest.Cluster) -> None:
    assert len(cluster.nodes) == 1

    prometheus_address = "0.0.0.0:9952"

    config = ReactorConfig(
            prometheus=[Prometheus(
                enums=True,
                address=prometheus_address
                )])
    reactortest.deploy_reactor(config, 'prometheus.toml', cluster.nodes)

    # match 'drbdreactor_up 1' exactly from prometheus output
    p = re.compile(r'drbdreactor_up\s1')

    for i in range(20):
        prom_out = reactortest.prometheus_endpoint_scrape(cluster.nodes[0], prometheus_address)
        if prom_out:
            m = p.search(prom_out)
            if m:
                reactortest.log(f'match found: "{m.group()}" in scrape {i}')
                break
            else:
                reactortest.log(f'no match found in scrape {i}')
        time.sleep(0.5)
    else:
        raise AssertionError('prometheus not responding correctly on {node}')
