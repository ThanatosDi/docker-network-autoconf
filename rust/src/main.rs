use std::collections::HashMap;
use std::env;

use bollard::Docker;
use bollard::errors::Error;
use bollard::models::{
    ContainerInspectResponse, EndpointSettings, EventMessageTypeEnum, Ipam, IpamConfig,
    NetworkConnectRequest, NetworkCreateRequest,
};
use bollard::query_parameters::EventsOptionsBuilder;
use futures_util::StreamExt;
use tracing::{debug, error, info, warn};
use tracing_subscriber::filter::Targets;
use tracing_subscriber::prelude::*;

/// Docker Network Autoconf 設定 labels（讀取自 docker-network-autoconf 容器本身）
mod setting_label {
    /// 需要自動建立的網路名稱，使用逗號分隔
    /// 例如: docker-network-autoconf.settings.networks=network1,network2
    pub const NETWORKS: &str = "docker-network-autoconf.settings.networks";

    /// 指定網路的子網段
    /// 例如: docker-network-autoconf.settings.networks.network1.subnet=89.207.132.170/24
    pub fn network_subnet(network: &str) -> String {
        format!("docker-network-autoconf.settings.networks.{network}.subnet")
    }

    /// 指定網路的閘道
    /// 例如: docker-network-autoconf.settings.networks.network1.gateway=192.168.2.126
    pub fn network_gateway(network: &str) -> String {
        format!("docker-network-autoconf.settings.networks.{network}.gateway")
    }
}

/// 其他專案的容器設定的 labels（讀取自需要自動連接網路的容器）
mod container_label {
    /// 該 container 是否套用自動連接網路功能，label 不存在時為 false
    /// 例如: docker-network-autoconf.enable=true
    pub const ENABLE: &str = "docker-network-autoconf.enable";

    /// 需要自動連接的網路名稱，使用逗號分隔
    /// 例如: docker-network-autoconf.networks=network1,network2
    pub const NETWORKS: &str = "docker-network-autoconf.networks";

    /// 指定網路的別名，使用逗號分隔
    /// 例如: docker-network-autoconf.networks.network1.aliases=alias1,alias2
    pub fn network_aliases(network: &str) -> String {
        format!("docker-network-autoconf.networks.{network}.aliases")
    }
}

fn labels(container: &ContainerInspectResponse) -> HashMap<String, String> {
    container
        .config
        .as_ref()
        .and_then(|config| config.labels.clone())
        .unwrap_or_default()
}

/// 讀取逗號分隔的 label，回傳去除空白且非空的值
fn get_container_label(labels: &HashMap<String, String>, label_key: &str) -> Vec<String> {
    labels
        .get(label_key)
        .map(String::as_str)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(String::from)
        .collect()
}

/// 檢查網路是否存在
async fn exists_network(client: &Docker, docker_network_name: &str) -> Result<bool, Error> {
    match client.inspect_network(docker_network_name, None).await {
        Ok(_) => Ok(true),
        Err(Error::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(false),
        Err(e) => Err(e),
    }
}

/// 依 docker-network-autoconf 容器的 labels 建立不存在的網路
async fn setup(client: &Docker, self_labels: &HashMap<String, String>) -> Result<(), Error> {
    info!("📡 檢查網路...");
    for network in get_container_label(self_labels, setting_label::NETWORKS) {
        if exists_network(client, &network).await? {
            continue;
        }
        warn!("⚠️ 網路 {network} 不存在，建立中...");
        let subnet = self_labels
            .get(&setting_label::network_subnet(&network))
            .cloned();
        let gateway = self_labels
            .get(&setting_label::network_gateway(&network))
            .cloned();
        debug!("🐞: Create network, network name: {network}, subnet: {subnet:?}");
        let request = NetworkCreateRequest {
            name: network.clone(),
            driver: Some("bridge".into()),
            ipam: Some(Ipam {
                config: Some(vec![IpamConfig {
                    subnet,
                    gateway,
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };
        match client.create_network(request).await {
            Ok(_) => info!("✅ 網路 {network} 建立完成"),
            Err(e) => error!("❌ 建立網路 {network} 失敗: {e}"),
        }
    }
    Ok(())
}

fn is_enabled(container: &ContainerInspectResponse) -> bool {
    let labels = labels(container);
    debug!("🐞 labels: {labels:?}");
    labels
        .get(container_label::ENABLE)
        .is_some_and(|enable| enable.eq_ignore_ascii_case("true"))
}

/// 連接容器到網路
///
/// 檢查容器是否已經連接到網路，如果已經連接，則跳過。
/// 連接容器到網路，並將容器的 alias 設定到網路中。
async fn connection(client: &Docker, container: &ContainerInspectResponse) -> Result<(), Error> {
    let container_id = container.id.as_deref().unwrap_or_default();
    let container_name = container
        .name
        .as_deref()
        .map(|name| name.trim_start_matches('/'))
        .unwrap_or(container_id);
    info!("ℹ️ 開始連接網路... container: {container_name}");

    let labels = labels(container);
    let networks = get_container_label(&labels, container_label::NETWORKS);
    let connected_networks = container
        .network_settings
        .as_ref()
        .and_then(|settings| settings.networks.clone())
        .unwrap_or_default();
    debug!(
        "🐞 networks: {networks:?}, connected_networks: {:?}",
        connected_networks.keys()
    );

    for label_network in networks {
        if !exists_network(client, &label_network).await? {
            error!("❌ 網路 {label_network} 不存在，跳過...");
            continue;
        }
        if connected_networks.contains_key(&label_network) {
            info!("ℹ️ 容器 {container_name} 已經連接到網路 {label_network}，跳過...");
            continue;
        }
        let network_aliases =
            get_container_label(&labels, &container_label::network_aliases(&label_network));
        debug!("🐞 Network aliases: {network_aliases:?}");
        let request = NetworkConnectRequest {
            container: container_id.to_string(),
            endpoint_config: Some(EndpointSettings {
                aliases: Some(network_aliases),
                ..Default::default()
            }),
        };
        client.connect_network(&label_network, request).await?;
        info!("✅ 容器 {container_name} 已連接到網路 {label_network}");
    }
    Ok(())
}

async fn handle_container_create(client: &Docker, container_id: &str) -> Result<(), Error> {
    let container = client.inspect_container(container_id, None).await?;
    if is_enabled(&container) {
        connection(client, &container).await?;
    }
    Ok(())
}

fn init_logger() {
    let level = match env::var("LOG_LEVEL")
        .unwrap_or_else(|_| "INFO".into())
        .to_uppercase()
        .as_str()
    {
        "TRACE" => tracing::Level::TRACE,
        "DEBUG" => tracing::Level::DEBUG,
        "WARN" | "WARNING" => tracing::Level::WARN,
        "ERROR" | "CRITICAL" => tracing::Level::ERROR,
        _ => tracing::Level::INFO,
    };
    // 只套用 LOG_LEVEL 到本程式，第三方套件（bollard 等）只輸出 WARN 以上
    let filter = Targets::new()
        .with_target(env!("CARGO_CRATE_NAME"), level)
        .with_default(tracing::Level::WARN);
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .init();
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    init_logger();

    let client = Docker::connect_with_defaults()?;
    let self_name = env::var("DOCKER_NETWORK_AUTOCONF_NAME")
        .unwrap_or_else(|_| "docker-network-autoconf".into());
    let self_container = client.inspect_container(&self_name, None).await?;

    if let Err(e) = setup(&client, &labels(&self_container)).await {
        error!("❌ 發生錯誤: {e}");
    }

    let filters = HashMap::from([("type", vec!["container"]), ("event", vec!["create"])]);
    let mut events = client.events(Some(EventsOptionsBuilder::new().filters(&filters).build()));

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            event = events.next() => {
                let Some(event) = event else { break };
                let event = event?;
                debug!(
                    "🐞 Event: {}",
                    serde_json::to_string_pretty(&event).unwrap_or_default()
                );
                if event.typ != Some(EventMessageTypeEnum::CONTAINER)
                    || event.action.as_deref() != Some("create")
                {
                    continue;
                }
                let Some(container_id) = event.actor.and_then(|actor| actor.id) else {
                    continue;
                };
                if let Err(e) = handle_container_create(&client, &container_id).await {
                    error!("❌ 發生錯誤: {e}");
                }
            }
        }
    }
    Ok(())
}
