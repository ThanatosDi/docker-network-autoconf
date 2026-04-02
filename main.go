package main

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"os"
	"strings"

	"github.com/docker/docker/api/types"
	"github.com/docker/docker/api/types/events"
	"github.com/docker/docker/api/types/filters"
	"github.com/docker/docker/api/types/network"
	"github.com/docker/docker/client"
)

const (
	// Settings labels (read from autoconf container itself)
	labelSettingsNetworks       = "docker-network-autoconf.settings.networks"
	labelSettingsNetworkSubnet  = "docker-network-autoconf.settings.networks.%s.subnet"
	labelSettingsNetworkGateway = "docker-network-autoconf.settings.networks.%s.gateway"

	// Container labels (read from target containers)
	labelEnable         = "docker-network-autoconf.enable"
	labelNetworks       = "docker-network-autoconf.networks"
	labelNetworkAliases = "docker-network-autoconf.networks.%s.aliases"
)

var (
	cli     *client.Client
	selfCtr types.ContainerJSON
	ctx     = context.Background()
)

func main() {
	initLogger()

	var err error
	cli, err = client.NewClientWithOpts(client.FromEnv, client.WithAPIVersionNegotiation())
	if err != nil {
		slog.Error("Failed to create Docker client", "error", err)
		os.Exit(1)
	}
	defer cli.Close()

	selfName := getEnv("DOCKER_NETWORK_AUTOCONF_NAME", "docker-network-autoconf")
	selfCtr, err = cli.ContainerInspect(ctx, selfName)
	if err != nil {
		slog.Error("Failed to inspect self container", "name", selfName, "error", err)
		os.Exit(1)
	}

	setup()
	listenEvents()
}

func initLogger() {
	level := slog.LevelInfo
	switch strings.ToUpper(getEnv("LOG_LEVEL", "INFO")) {
	case "DEBUG":
		level = slog.LevelDebug
	case "WARN", "WARNING":
		level = slog.LevelWarn
	case "ERROR":
		level = slog.LevelError
	}
	slog.SetDefault(slog.New(slog.NewTextHandler(os.Stdout, &slog.HandlerOptions{Level: level})))
}

func getEnv(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

// getContainerLabel reads a comma-separated label from a container and returns trimmed, non-empty values.
func getContainerLabel(containerID, labelKey string) ([]string, error) {
	ctr, err := cli.ContainerInspect(ctx, containerID)
	if err != nil {
		return nil, err
	}
	raw := ctr.Config.Labels[labelKey]
	if raw == "" {
		return nil, nil
	}
	parts := strings.Split(raw, ",")
	var result []string
	for _, p := range parts {
		p = strings.TrimSpace(p)
		if p != "" {
			result = append(result, p)
		}
	}
	return result, nil
}

// existsNetwork checks if a Docker network exists by name.
func existsNetwork(name string) bool {
	_, err := cli.NetworkInspect(ctx, name, network.InspectOptions{})
	return err == nil
}

// setup creates networks defined in the autoconf container's labels.
func setup() {
	slog.Info("Checking networks...")
	raw := selfCtr.Config.Labels[labelSettingsNetworks]
	if raw == "" {
		slog.Info("No networks configured")
		return
	}

	networks := strings.Split(raw, ",")
	for _, net := range networks {
		net = strings.TrimSpace(net)
		if net == "" {
			continue
		}
		if existsNetwork(net) {
			slog.Info("Network already exists", "network", net)
			continue
		}

		slog.Warn("Network does not exist, creating...", "network", net)
		subnet := selfCtr.Config.Labels[fmt.Sprintf(labelSettingsNetworkSubnet, net)]
		gateway := selfCtr.Config.Labels[fmt.Sprintf(labelSettingsNetworkGateway, net)]

		ipamConfig := network.IPAM{
			Config: []network.IPAMConfig{
				{
					Subnet:  subnet,
					Gateway: gateway,
				},
			},
		}

		slog.Debug("Creating network", "network", net, "subnet", subnet, "gateway", gateway)

		_, err := cli.NetworkCreate(ctx, net, network.CreateOptions{
			Driver: "bridge",
			IPAM:   &ipamConfig,
		})
		if err != nil {
			slog.Error("Failed to create network", "network", net, "error", err)
			continue
		}
		slog.Info("Network created", "network", net)
	}
}

// isEnabled checks if a container has the autoconf enable label set to "true".
func isEnabled(containerID string) bool {
	ctr, err := cli.ContainerInspect(ctx, containerID)
	if err != nil {
		slog.Error("Failed to inspect container", "container", containerID, "error", err)
		return false
	}
	slog.Debug("Container labels", "labels", ctr.Config.Labels)
	return strings.EqualFold(ctr.Config.Labels[labelEnable], "true")
}

// connection connects a container to the networks specified in its labels.
func connection(containerID string) {
	slog.Info("Connecting container to networks...", "container", containerID)

	ctr, err := cli.ContainerInspect(ctx, containerID)
	if err != nil {
		slog.Error("Failed to inspect container", "container", containerID, "error", err)
		return
	}

	networks, err := getContainerLabel(ctr.ID, labelNetworks)
	if err != nil {
		slog.Error("Failed to get container networks label", "container", containerID, "error", err)
		return
	}

	connectedNetworks := ctr.NetworkSettings.Networks
	slog.Debug("Network info", "networks", networks, "connected", connectedNetworks)

	for _, labelNetwork := range networks {
		if !existsNetwork(labelNetwork) {
			slog.Error("Network does not exist, skipping...", "network", labelNetwork)
			continue
		}

		if _, ok := connectedNetworks[labelNetwork]; ok {
			slog.Info("Container already connected to network, skipping...",
				"container", ctr.Name, "network", labelNetwork)
			return
		}

		aliases, err := getContainerLabel(ctr.ID, fmt.Sprintf(labelNetworkAliases, labelNetwork))
		if err != nil {
			slog.Error("Failed to get network aliases", "network", labelNetwork, "error", err)
			continue
		}
		slog.Debug("Network aliases", "aliases", aliases)

		err = cli.NetworkConnect(ctx, labelNetwork, ctr.ID, &network.EndpointSettings{
			Aliases: aliases,
		})
		if err != nil {
			slog.Error("Failed to connect container to network",
				"container", ctr.Name, "network", labelNetwork, "error", err)
			continue
		}
		slog.Info("Connected container to network", "container", ctr.Name, "network", labelNetwork)
	}
}

// listenEvents listens for Docker container create events and processes them.
func listenEvents() {
	slog.Info("Listening for Docker events...")

	eventFilter := filters.NewArgs()
	eventFilter.Add("type", string(events.ContainerEventType))
	eventFilter.Add("event", "create")

	msgCh, errCh := cli.Events(ctx, events.ListOptions{Filters: eventFilter})

	for {
		select {
		case event := <-msgCh:
			eventJSON, _ := json.MarshalIndent(event, "", "  ")
			slog.Debug("Event received", "event", string(eventJSON))

			containerID := event.Actor.ID
			if isEnabled(containerID) {
				connection(containerID)
			}

		case err := <-errCh:
			if err != nil {
				slog.Error("Event stream error", "error", err)
				os.Exit(1)
			}
			return
		}
	}
}
