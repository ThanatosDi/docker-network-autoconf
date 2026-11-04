# Docker Network Autoconf
<img width="1024" height="559" alt="image" src="https://github.com/user-attachments/assets/f9af8f3d-c730-4f95-b1d9-459690e3f700" />

## What is this?

`Docker Network Autoconf` is an automated service that continuously listens for Docker events. When a new container is created, it automatically connects the container to specified Docker networks based on its labels.

This tool aims to simplify network configuration in a Docker environment, especially when managing multiple interdependent services with `docker compose`. It significantly reduces the complexity and potential for errors associated with manual setup.

## Core Features

- **Automatic Network Creation**: On startup, the service automatically checks for and creates the networks specified in its configuration, with support for custom subnets and gateways.
- **Automatic Container Connection**: It listens for newly created containers in real-time and, if a container is marked as enabled, connects it to the appropriate networks.
- **Label-Based Configuration**: All settings are managed via Docker labels, eliminating the need to modify configuration files or restart the service.
- **Network Alias Support**: You can assign one or more network aliases to a container when it connects to a network.
- **Event-Driven**: It monitors the Docker API for events, making it fast and resource-efficient.

## How It Works

1.  **Startup and Initialization**:
    -   After the service (`python/main.py`) starts, it first reads the labels on its **own container**.
    -   Based on the `docker-network-autoconf.settings.networks` label, it identifies which networks to manage.
    -   The service checks if these networks exist. If not, it creates them using the corresponding `subnet` and `gateway` label settings.

2.  **Event Listening**:
    -   Once initialization is complete, the service begins listening for real-time Docker events (`client.events`).
    -   It specifically listens for the `container create` event.

3.  **Handling New Containers**:
    -   When a new container is created, the service captures this event.
    -   It checks if the container has the `docker-network-autoconf.enable: "true"` label.
    -   If enabled, the service reads the container's `docker-network-autoconf.networks` label to determine which networks it should connect to.
    -   Finally, the service connects the container to each specified network and applies any corresponding network aliases (`aliases`).

## Usage

The best practice is to configure this service and your other containers using a `docker-compose.yml` file.

Here is a complete example:

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
      
  # This is the network automation service itself
  docker-network-autoconf:
    # build: .  # If you prefer to build the image yourself
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
    # Configure the tool's behavior via labels
    labels:
      # Define the list of networks to manage
      - "docker-network-autoconf.settings.networks=frontend,backend"
      # (Optional) Configure subnet and gateway for the 'backend' network
      - "docker-network-autoconf.settings.networks.backend.subnet=172.20.0.0/24"
      - "docker-network-autoconf.settings.networks.backend.gateway=172.20.0.1"

---

services:
  # This is a service you want to automatically connect (e.g., Nginx)
  nginx:
    image: nginx:latest
    restart: unless-stopped
    ports:
      - "80:80"
    # Use labels to tell the autoconf service how to handle this container
    labels:
      # Enable the auto-connect feature
      - "docker-network-autoconf.enable=true"
      # Specify which networks to connect to
      - "docker-network-autoconf.networks=frontend,backend"
      # (Optional) Set aliases on the 'frontend' network
      - "docker-network-autoconf.networks.frontend.aliases=nginx-proxy,web"

  # This is another service that needs to connect to the backend network
  api-server:
    image: your-api-image:latest
    restart: unless-stopped
    labels:
      - "docker-network-autoconf.enable=true"
      - "docker-network-autoconf.networks=backend"
      # (Optional) Set an alias on the 'backend' network
      - "docker-network-autoconf.networks.backend.aliases=api"

```

## Label Reference

#### 1. Autoconf Service Configuration (Set on the `docker-network-autoconf` container)

-   `docker-network-autoconf.settings.networks`
    -   **Description**: Defines a comma-separated list of networks for this tool to manage (check or create).
    -   **Example**: `"network1,network2,network3"`

-   `docker-network-autoconf.settings.networks.{network}.subnet`
    -   **Description**: (Optional) Sets the subnet for a specified network. This is used when the network is created.
    -   **Example**: `"docker-network-autoconf.settings.networks.network1.subnet=192.168.10.0/24"`

-   `docker-network-autoconf.settings.networks.{network}.gateway`
    -   **Description**: (Optional) Sets the gateway for a specified network. This is used when the network is created.
    -   **Example**: `"docker-network-autoconf.settings.networks.network1.gateway=192.168.10.1"`

#### 2. Target Container Configuration (Set on the services you want to manage)

-   `docker-network-autoconf.enable`
    -   **Description**: Enables the auto-connect feature for this container. Must be set to `"true"` to take effect.
    -   **Example**: `"true"`
    -   **Default**: `"false"`

-   `docker-network-autoconf.networks`
    -   **Description**: A comma-separated list of networks this container should connect to.
    -   **Example**: `"frontend,backend"`

-   `docker-network-autoconf.networks.{network}.aliases`
    -   **Description**: (Optional) A comma-separated list of aliases for the container on a specific network. Other containers can use these aliases to reach this container.
    -   **Example**: `"docker-network-autoconf.networks.frontend.aliases=web,proxy"`

## Security Recommendation

Mounting the Docker Socket (`/var/run/docker.sock`) directly into a container grants it extensive privileges. It is strongly recommended to use [linuxserver/socket-proxy](https://github.com/linuxserver/docker-socket-proxy) to limit the `docker-network-autoconf` container's access to only the necessary Docker API endpoints, thereby enhancing security.
