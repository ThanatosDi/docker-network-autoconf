# Docker Network Autoconf

## 這是什麼？

`Docker Network Autoconf` 是一個自動化服務，它會持續監聽 Docker 的事件。當有新的容器被建立時，它會根據容器上的標籤（Labels），自動將該容器連接到指定的 Docker 網路上。

這款工具旨在簡化 Docker 環境中的網路配置，特別是在使用 `docker compose` 管理多個互相依賴的服務時，能大幅減少手動設定的複雜性並避免錯誤。

## 核心特色

- **自動建立網路**：服務啟動時，會自動檢查並建立設定檔中指定的網路，支援自訂子網路（Subnet）和閘道（Gateway）。
- **自動連接容器**：即時監聽新建立的容器，若容器標示為啟用，則自動將其連接到對應的網路。
- **基於標籤的設定**：所有設定都透過 Docker 標籤進行，無需修改設定檔或重啟服務。
- **支援網路別名**：在連接網路時，可以為容器指定一個或多個網路別名（Aliases）。
- **事件驅動**：監控 Docker API 事件，反應迅速且資源佔用低。

## 運作原理

1.  **啟動與初始化**：
    - 服務啟動後，首先會讀取其**自身容器**上的標籤。
    - 根據 `docker-network-autoconf.settings.networks` 標籤，它會得知需要管理哪些網路。
    - 服務會檢查這些網路是否存在，如果不存在，則會使用對應的 `subnet` 和 `gateway` 標籤設定來自動建立網路。

2.  **監聽事件**：
    - 初始化完成後，服務會開始監聽 Docker 的即時事件。
    - 它特別關注 `container` 的 `create` 事件。

3.  **處理新容器**：
    - 當一個新的容器被建立時，服務會捕捉到這個事件。
    - 它會檢查該容器是否包含 `docker-network-autoconf.enable: "true"` 標籤。
    - 如果啟用，服務會讀取該容器的 `docker-network-autoconf.networks` 標籤，以確定它需要連接哪些網路。
    - 最後，服務會將該容器連接到每一個指定的網路，並設定對應的網路別名（`aliases`）。

## 使用方法

最佳實踐是透過 `docker-compose.yml` 來設定此服務以及您想自動管理的其他容器。

以下是一個完整的範例：

```yaml
services:
  socket-proxy:
    image: lscr.io/linuxserver/socket-proxy:latest
    container_name: socket-proxy
    restart: unless-stopped
    environment:
      - CONTAINERS=1
      - VERSION=1
      - INFO=1
      # Required for container connect network
      - POST=1
      - NETWORKS=1
      - TZ=${TIMEZONE:-UTC}
    volumes:
      - ${DOCKET_SOCKET:-/var/run/docker.sock}:/var/run/docker.sock:ro
    read_only: true
    tmpfs:
      - /run
    networks:
      - socket-proxy
  # 這是自動化網路設定服務本身
  docker-network-autoconf:
    # build: .  # 如果您要自行建置映像檔
    image: thanatosdi/docker-network-autoconf:latest
    container_name: ${CONTAINER_NAME:-docker-network-autoconf}
    restart: unless-stopped
    environment:
      - DOCKER_HOST=tcp://socket-proxy:2375
      - DOCKER_NETWORK_AUTOCONF_NAME=${CONTAINER_NAME:-docker-network-autoconf}
      - LOG_LEVEL=${LOG_LEVEL:-INFO}
      - CONTAINER_NAME=${CONTAINER_NAME:-docker-network-autoconf}
    networks:
      - socket-proxy
    depends_on:
      - socket-proxy
    # 透過標籤設定此工具的行為
    labels:
      # 定義需要被管理的網路清單
      - "docker-network-autoconf.settings.networks=frontend,backend"
      # (可選) 為 'backend' 網路設定子網路和閘道
      - "docker-network-autoconf.settings.networks.backend.subnet=172.20.0.0/24"
      - "docker-network-autoconf.settings.networks.backend.gateway=172.20.0.1"

---

services:
  # 這是您想要自動連接網路的其中一個服務 (例如 Nginx)
  nginx:
    image: nginx:latest
    restart: unless-stopped
    ports:
      - "80:80"
    # 透過標籤讓 autoconf 服務知道如何處理這個容器
    labels:
      # 啟用自動連接功能
      - "docker-network-autoconf.enable=true"
      # 指定要連接的網路
      - "docker-network-autoconf.networks=frontend,backend"
      # (可選) 在 'frontend' 網路上設定別名
      - "docker-network-autoconf.networks.frontend.aliases=nginx-proxy,web"

  # 這是另一個需要連接到後端網路的服務
  api-server:
    image: your-api-image:latest
    restart: unless-stopped
    labels:
      - "docker-network-autoconf.enable=true"
      - "docker-network-autoconf.networks=backend"
      # (可選) 在 'backend' 網路上設定別名
      - "docker-network-autoconf.networks.backend.aliases=api"

```

## 標籤參考

#### 1. 本工具容器設定 (在 `docker-network-autoconf` 服務上設定)

-   `docker-network-autoconf.settings.networks`
    -   **說明**：定義此工具需要管理（檢查或建立）的網路清單，以逗號分隔。
    -   **範例**：`"network1,network2,network3"`

-   `docker-network-autoconf.settings.networks.{network}.subnet`
    -   **說明**：(可選) 為指定的網路設定子網路。如果網路不存在，建立時將會使用此設定。
    -   **範例**：`"docker-network-autoconf.settings.networks.network1.subnet=192.168.10.0/24"`

-   `docker-network-autoconf.settings.networks.{network}.gateway`
    -   **說明**：(可選) 為指定的網路設定閘道。如果網路不存在，建立時將會使用此設定。
    -   **範例**：`"docker-network-autoconf.settings.networks.network1.gateway=192.168.10.1"`

#### 2. 其他目標容器設定 (在您想自動化管理的服務上設定)

-   `docker-network-autoconf.enable`
    -   **說明**：是否為此容器啟用自動網路連接功能。必須設為 `"true"` 才會生效。
    -   **範例**：`"true"`
    -   **預設值**: `"false"`

-   `docker-network-autoconf.networks`
    -   **說明**：指定此容器需要連接的網路清單，以逗號分隔。
    -   **範例**：`"frontend,backend"`

-   `docker-network-autoconf.networks.{network}.aliases`
    -   **說明**：(可選) 指定容器在某個特定網路上使用的別名，以逗號分隔。其他容器可以使用這些別名來訪問此容器。
    -   **範例**：`"docker-network-autoconf.networks.frontend.aliases=web,proxy"`

## 安全建議

直接將 Docker Socket (`/var/run/docker.sock`) 掛載到容器中會給予容器過高的權限。強烈建議搭配 [linuxserver/socket-proxy](https://github.com/linuxserver/docker-socket-proxy) 使用，以限制 `docker-network-autoconf` 容器只能存取它所需要的 Docker API 功能，從而提高安全性。