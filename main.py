import json
import os
import sys
from enum import Enum

import docker
from docker.types import IPAMConfig, IPAMPool
from loguru import logger


class SettingEnum(str, Enum):
    """
    Docker Network Autoconf 設定列舉
    這些設定用於從容器標籤中讀取網路設定
    """

    NETWORKS = "docker-network-autoconf.settings.networks"
    """
    networks: 定義需要自動建立的網路名稱，使用逗號分隔
    例如: "network1,network2"
    例如標籤:
    docker-network-autoconf.settings.networks=network1,network2
    代表需要建立兩個網路，分別為 network1 和 network2
    這些網路名稱將用於後續的子網路和閘道設定
    例如標籤:
    docker-network-autoconf.settings.networks=network1,network2
    """
    NETWORK_SUBNET = "docker-network-autoconf.settings.networks.{network}.subnet"
    """
    network.{network}.subnet: 定義指定網路的子網段
    例如: "network.network1.subnet=89.207.132.170/24"
    代表網路 network1 的子網段為 89.207.132.170/24
    例如標籤:
    docker-network-autoconf.settings.networks.network1.subnet=89.207.132.170/24
    """
    NETWORK_GATEWAY = "docker-network-autoconf.settings.networks.{network}.gateway"
    """
    network.{network}.gateway: 定義指定網路的閘道
    例如: "network.network1.gateway=192.168.2.126"
    代表網路 network1 的閘道為 192.168.2.126
    例如標籤:
    docker-network-autoconf.settings.networks.network1.gateway=192.168.2.126
    """


class ContainerLabelEnum(str, Enum):
    """
    其他專案的容器設定的 Labels 列舉
    這些設定用於從其他容器標籤中讀取自動連接網路的設定
    例如啟用自動連接功能、指定需要連接的網路及其別名
    例如標籤:
    docker-network-autoconf.enable=true
    docker-network-autoconf.networks=network1,network2
    docker-network-autoconf.networks.network1.aliases=alias1,alias2
    """

    ENABLE = "docker-network-autoconf.enable"
    """
    enable: 該 container 是否套用自動連接網路功能
    預設該 label 不存在時為 false
    例如:
    docker-network-autoconf.enable=true
    """
    NETWORKS = "docker-network-autoconf.networks"
    """
    networks: 定義需要自動連接的網路名稱，使用逗號分隔
    例如: "network1,network2"
    例如標籤:
    docker-network-autoconf.networks=network1,network2
    代表該容器需要連接到兩個網路，分別為 network1 和 network2
    這些網路名稱將用於後續的別名設定
    """
    NETWORK_ALIASES = "docker-network-autoconf.networks.{network}.aliases"
    """
    networks.{network}.aliases: 定義指定網路的別名，使用逗號分隔
    例如: "network.network1.aliases=alias1,alias2"
    代表該容器在連接到 network1 網路時，會使用 alias1 和 alias2 作為別名
    例如標籤:
    docker-network-autoconf.networks.network1.aliases=alias1,alias2
    """


client = docker.from_env()
logger.remove()
logger.add(sys.stdout, level=os.getenv("LOG_LEVEL", "INFO").upper())

self = client.containers.get(
    os.getenv("DOCKER_NETWORK_AUTOCONF_NAME", "docker-network-autoconf")
)


def __get_container_label(container_id_or_name: str, label_key: str) -> list[str]:
    container = client.containers.get(container_id_or_name)
    labels = list(
        filter(
            None,
            container.labels.get(label_key, "").split(","),
        )
    )
    return [label.strip() for label in labels]


def __exists_network(docker_network_name: str):
    """
    檢查網路是否存在

    Args:
        docker_network_name (str): 網路名稱

    Returns:
        bool: 網路是否存在
    """
    try:
        client.networks.get(docker_network_name)
        return True
    except docker.errors.NotFound:
        return False


@logger.catch
def setup():
    """ """
    logger.info("📡 檢查網路...")
    networks = self.labels.get(SettingEnum.NETWORKS.value, "").split(",")
    for network in networks:
        if not __exists_network(network):
            logger.warning(f"⚠️ 網路 {network} 不存在，建立中...")
            subnet = self.labels.get(
                SettingEnum.NETWORK_SUBNET.value.format(network=network), None
            )
            gateway = self.labels.get(
                SettingEnum.NETWORK_GATEWAY.value.format(network=network), None
            )
            ipam_config = IPAMConfig(
                pool_configs=[
                    IPAMPool(
                        subnet=subnet,
                        gateway=gateway,
                    )
                ]
            )
            logger.debug(
                f"🐞: Create network, network name: {network}, subnet: {subnet}"
            )
            try:
                client.networks.create(network, ipam=ipam_config, driver="bridge")
                logger.info(f"✅ 網路 {network} 建立完成")
            except Exception as e:
                logger.error(f"❌ 建立網路 {network} 失敗: {e}")
                continue


@logger.catch
def is_enabled(container_id_or_name: str):
    container = client.containers.get(container_id_or_name)
    logger.debug(f"🐞 labels: {container.labels}")
    return (
        True
        if container.labels.get(ContainerLabelEnum.ENABLE.value, "false").lower()
        == "true"
        else False
    )


@logger.catch
def connection(container_id_or_name: str):
    """
    連接容器到網路

    Args:
        container_id_or_name (str): 容器 id 或名稱

    Returns:
        None

    Notes:
        連接容器到網路，檢查容器是否已經連接到網路，如果已經連接，則跳過。
        連接容器到網路，並將容器的 alias 設定到網路中。
    """
    logger.info(f"ℹ️ 開始連接網路... container_id_or_name: {container_id_or_name}")
    container = client.containers.get(container_id_or_name)
    networks = __get_container_label(container.id, ContainerLabelEnum.NETWORKS.value)
    connected_networks = container.attrs["NetworkSettings"]["Networks"]
    logger.debug(f"🐞 networks: {networks}, connected_networks:{connected_networks}")
    for label_network in networks:
        try:
            network = client.networks.get(label_network)
        except docker.errors.NotFound:
            logger.error(f"❌ 網路 {label_network} 不存在，跳過...")
            continue
        if network in connected_networks:
            logger.info(f"ℹ️ 容器 {container.name} 已經連接到網路 {network}，跳過...")
            return
        network_aliases = __get_container_label(
            container.id,
            ContainerLabelEnum.NETWORK_ALIASES.value.format(network=label_network),
        )
        logger.debug(f"🐞 Network aliases: {network_aliases}")
        network.connect(container, aliases=network_aliases)


if __name__ == "__main__":
    setup()
    event: dict
    try:
        for event in client.events(decode=True):
            try:
                logger.debug(f"🐞 Event: {json.dumps(event, indent=2)}")
                event_type = event.get("Type")
                event_action = event.get("Action")
                match (event_type, event_action):
                    case ("container", "create"):
                        container_id = event["Actor"]["ID"]
                        if is_enabled(container_id):
                            connection(container_id)
                    case _:
                        continue
            except Exception as e:
                print(f"❌ 發生錯誤: {e}")
                continue
    except KeyboardInterrupt:
        pass
