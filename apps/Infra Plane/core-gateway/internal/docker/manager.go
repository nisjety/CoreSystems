package docker

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"time"

	"github.com/coresystem/integration-gateway/internal/audit"
	"github.com/coresystem/integration-gateway/internal/config"
	"github.com/docker/docker/api/types/container"
	"github.com/docker/docker/api/types/filters"
	"github.com/docker/docker/api/types/image"
	"github.com/docker/docker/api/types/network"
	"github.com/docker/docker/api/types/system"
	"github.com/docker/docker/api/types/volume"
	"github.com/docker/docker/client"
)

// Manager handles Docker operations via socket proxy
type Manager struct {
	client *client.Client
	audit  *audit.AuditLogger
}

// NewManager creates a new Docker manager
func NewManager(cfg config.DockerConfig, auditLogger *audit.AuditLogger) (*Manager, error) {
	cli, err := client.NewClientWithOpts(
		client.WithHost(cfg.Host),
		client.WithVersion(cfg.Version),
		client.WithTimeout(cfg.Timeout),
	)
	if err != nil {
		return nil, fmt.Errorf("failed to create Docker client: %w", err)
	}

	return &Manager{
		client: cli,
		audit:  auditLogger,
	}, nil
}

// Close closes the Docker client connection
func (m *Manager) Close() error {
	return m.client.Close()
}

// ContainerInfo represents container information
type ContainerInfo struct {
	ID      string            `json:"id"`
	Name    string            `json:"name"`
	Image   string            `json:"image"`
	State   string            `json:"state"`
	Status  string            `json:"status"`
	Created int64             `json:"created"`
	Ports   []PortBinding     `json:"ports"`
	Labels  map[string]string `json:"labels"`
}

// PortBinding represents a container port binding
type PortBinding struct {
	PrivatePort uint16 `json:"private_port"`
	PublicPort  uint16 `json:"public_port,omitempty"`
	Type        string `json:"type"`
}

// ContainerStats represents container resource stats
type ContainerStats struct {
	ID            string  `json:"id"`
	Name          string  `json:"name"`
	CPUPercent    float64 `json:"cpu_percent"`
	MemoryUsage   uint64  `json:"memory_usage"`
	MemoryLimit   uint64  `json:"memory_limit"`
	MemoryPercent float64 `json:"memory_percent"`
	NetworkRx     uint64  `json:"network_rx"`
	NetworkTx     uint64  `json:"network_tx"`
	BlockRead     uint64  `json:"block_read"`
	BlockWrite    uint64  `json:"block_write"`
}

// ListContainers lists all containers (including stopped ones)
func (m *Manager) ListContainers(ctx context.Context) ([]ContainerInfo, error) {
	containers, err := m.client.ContainerList(ctx, container.ListOptions{
		All: true,
	})
	if err != nil {
		return nil, fmt.Errorf("failed to list containers: %w", err)
	}

	result := make([]ContainerInfo, 0, len(containers))
	for _, c := range containers {
		// Extract container name (remove leading /)
		name := ""
		if len(c.Names) > 0 {
			name = c.Names[0][1:] // Remove leading /
		}

		// Convert ports
		ports := make([]PortBinding, 0, len(c.Ports))
		for _, p := range c.Ports {
			ports = append(ports, PortBinding{
				PrivatePort: p.PrivatePort,
				PublicPort:  p.PublicPort,
				Type:        p.Type,
			})
		}

		result = append(result, ContainerInfo{
			ID:      c.ID[:12], // Short ID
			Name:    name,
			Image:   c.Image,
			State:   c.State,
			Status:  c.Status,
			Created: c.Created,
			Ports:   ports,
			Labels:  c.Labels,
		})
	}

	return result, nil
}

// GetContainer gets detailed information about a specific container
func (m *Manager) GetContainer(ctx context.Context, nameOrID string) (*ContainerInfo, error) {
	// Get container details
	inspect, err := m.client.ContainerInspect(ctx, nameOrID)
	if err != nil {
		return nil, fmt.Errorf("failed to inspect container: %w", err)
	}

	// Convert port bindings
	ports := make([]PortBinding, 0)
	for port, bindings := range inspect.NetworkSettings.Ports {
		for _, binding := range bindings {
			privatePort := uint16(port.Int())
			publicPort := uint16(0)
			if binding.HostPort != "" {
				fmt.Sscanf(binding.HostPort, "%d", &publicPort)
			}
			ports = append(ports, PortBinding{
				PrivatePort: privatePort,
				PublicPort:  publicPort,
				Type:        port.Proto(),
			})
		}
	}

	// Parse Created timestamp
	createdTime, err := time.Parse(time.RFC3339Nano, inspect.Created)
	var createdUnix int64
	if err == nil {
		createdUnix = createdTime.Unix()
	}

	return &ContainerInfo{
		ID:      inspect.ID[:12],
		Name:    inspect.Name[1:], // Remove leading /
		Image:   inspect.Config.Image,
		State:   inspect.State.Status,
		Status:  inspect.State.Status,
		Created: createdUnix,
		Ports:   ports,
		Labels:  inspect.Config.Labels,
	}, nil
}

// StartContainer starts a stopped container
func (m *Manager) StartContainer(ctx context.Context, nameOrID string) error {
	err := m.client.ContainerStart(ctx, nameOrID, container.StartOptions{})
	if err != nil {
		return fmt.Errorf("failed to start container: %w", err)
	}

	if m.audit != nil {
		m.audit.LogEvent(audit.AuditEvent{
			Timestamp: time.Now(),
			Action:    "docker_container_start",
			Actor:     "gateway",
			Resource:  nameOrID,
			Success:   true,
		})
	}

	return nil
}

// StopContainer stops a running container
func (m *Manager) StopContainer(ctx context.Context, nameOrID string, timeout int) error {
	stopTimeout := timeout
	err := m.client.ContainerStop(ctx, nameOrID, container.StopOptions{
		Timeout: &stopTimeout,
	})
	if err != nil {
		return fmt.Errorf("failed to stop container: %w", err)
	}

	if m.audit != nil {
		m.audit.LogEvent(audit.AuditEvent{
			Timestamp: time.Now(),
			Action:    "docker_container_stop",
			Actor:     "gateway",
			Resource:  nameOrID,
			Success:   true,
		})
	}

	return nil
}

// RestartContainer restarts a container
func (m *Manager) RestartContainer(ctx context.Context, nameOrID string, timeout int) error {
	restartTimeout := timeout
	err := m.client.ContainerRestart(ctx, nameOrID, container.StopOptions{
		Timeout: &restartTimeout,
	})
	if err != nil {
		return fmt.Errorf("failed to restart container: %w", err)
	}

	if m.audit != nil {
		m.audit.LogEvent(audit.AuditEvent{
			Timestamp: time.Now(),
			Action:    "docker_container_restart",
			Actor:     "gateway",
			Resource:  nameOrID,
			Success:   true,
		})
	}

	return nil
}

// GetContainerLogs retrieves container logs
func (m *Manager) GetContainerLogs(ctx context.Context, nameOrID string, tail string, since string) (string, error) {
	options := container.LogsOptions{
		ShowStdout: true,
		ShowStderr: true,
		Tail:       tail,
		Timestamps: true,
	}

	if since != "" {
		options.Since = since
	}

	logs, err := m.client.ContainerLogs(ctx, nameOrID, options)
	if err != nil {
		return "", fmt.Errorf("failed to get container logs: %w", err)
	}
	defer logs.Close()

	// Read all logs
	logBytes, err := io.ReadAll(logs)
	if err != nil {
		return "", fmt.Errorf("failed to read logs: %w", err)
	}

	return string(logBytes), nil
}

// GetContainerStats retrieves real-time container statistics
func (m *Manager) GetContainerStats(ctx context.Context, nameOrID string) (*ContainerStats, error) {
	stats, err := m.client.ContainerStats(ctx, nameOrID, false)
	if err != nil {
		return nil, fmt.Errorf("failed to get container stats: %w", err)
	}
	defer stats.Body.Close()

	var containerStats container.StatsResponse
	if err := json.NewDecoder(stats.Body).Decode(&containerStats); err != nil {
		return nil, fmt.Errorf("failed to decode stats: %w", err)
	}

	// Calculate CPU percentage
	cpuPercent := 0.0
	if containerStats.PreCPUStats.SystemUsage != 0 {
		cpuDelta := float64(containerStats.CPUStats.CPUUsage.TotalUsage - containerStats.PreCPUStats.CPUUsage.TotalUsage)
		systemDelta := float64(containerStats.CPUStats.SystemUsage - containerStats.PreCPUStats.SystemUsage)
		if systemDelta > 0 {
			cpuPercent = (cpuDelta / systemDelta) * float64(len(containerStats.CPUStats.CPUUsage.PercpuUsage)) * 100.0
		}
	}

	// Calculate memory percentage
	memPercent := 0.0
	if containerStats.MemoryStats.Limit > 0 {
		memPercent = float64(containerStats.MemoryStats.Usage) / float64(containerStats.MemoryStats.Limit) * 100.0
	}

	// Sum network I/O
	var networkRx, networkTx uint64
	for _, network := range containerStats.Networks {
		networkRx += network.RxBytes
		networkTx += network.TxBytes
	}

	// Sum block I/O
	var blockRead, blockWrite uint64
	for _, stat := range containerStats.BlkioStats.IoServiceBytesRecursive {
		if stat.Op == "read" {
			blockRead += stat.Value
		} else if stat.Op == "write" {
			blockWrite += stat.Value
		}
	}

	// Get container info for name
	inspect, err := m.client.ContainerInspect(ctx, nameOrID)
	if err != nil {
		return nil, fmt.Errorf("failed to inspect container: %w", err)
	}

	return &ContainerStats{
		ID:            nameOrID[:12],
		Name:          inspect.Name[1:],
		CPUPercent:    cpuPercent,
		MemoryUsage:   containerStats.MemoryStats.Usage,
		MemoryLimit:   containerStats.MemoryStats.Limit,
		MemoryPercent: memPercent,
		NetworkRx:     networkRx,
		NetworkTx:     networkTx,
		BlockRead:     blockRead,
		BlockWrite:    blockWrite,
	}, nil
}

// ListImages lists all Docker images
func (m *Manager) ListImages(ctx context.Context) ([]image.Summary, error) {
	images, err := m.client.ImageList(ctx, image.ListOptions{
		All: true,
	})
	if err != nil {
		return nil, fmt.Errorf("failed to list images: %w", err)
	}
	return images, nil
}

// GetSystemInfo returns Docker system information
func (m *Manager) GetSystemInfo(ctx context.Context) (*system.Info, error) {
	info, err := m.client.Info(ctx)
	if err != nil {
		return nil, fmt.Errorf("failed to get system info: %w", err)
	}
	return &info, nil
}

// Ping checks if Docker daemon is responding
func (m *Manager) Ping(ctx context.Context) error {
	_, err := m.client.Ping(ctx)
	return err
}

// ListNetworks returns all Docker networks
func (m *Manager) ListNetworks(ctx context.Context) ([]network.Summary, error) {
	networks, err := m.client.NetworkList(ctx, network.ListOptions{})
	if err != nil {
		return nil, fmt.Errorf("failed to list networks: %w", err)
	}
	return networks, nil
}

// ListVolumes lists all Docker volumes
func (m *Manager) ListVolumes(ctx context.Context) ([]*volume.Volume, error) {
	volumes, err := m.client.VolumeList(ctx, volume.ListOptions{})
	if err != nil {
		return nil, fmt.Errorf("failed to list volumes: %w", err)
	}
	return volumes.Volumes, nil
}

// GetAquatiqServices returns information about Aquatiq-specific services
func (m *Manager) GetAquatiqServices(ctx context.Context) ([]ContainerInfo, error) {
	// Filter for containers with coresystem label or name prefix
	filterArgs := filters.NewArgs()
	filterArgs.Add("name", "coresystem-")

	containers, err := m.client.ContainerList(ctx, container.ListOptions{
		All:     true,
		Filters: filterArgs,
	})
	if err != nil {
		return nil, fmt.Errorf("failed to list Aquatiq services: %w", err)
	}

	result := make([]ContainerInfo, 0, len(containers))
	for _, c := range containers {
		name := ""
		if len(c.Names) > 0 {
			name = c.Names[0][1:]
		}

		ports := make([]PortBinding, 0, len(c.Ports))
		for _, p := range c.Ports {
			ports = append(ports, PortBinding{
				PrivatePort: p.PrivatePort,
				PublicPort:  p.PublicPort,
				Type:        p.Type,
			})
		}

		result = append(result, ContainerInfo{
			ID:      c.ID[:12],
			Name:    name,
			Image:   c.Image,
			State:   c.State,
			Status:  c.Status,
			Created: c.Created,
			Ports:   ports,
			Labels:  c.Labels,
		})
	}

	return result, nil
}
